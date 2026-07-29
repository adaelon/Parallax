use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::Duration,
};

use eam_core::{
    ClaimOwner, CoreError, InMemoryRepository, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, RuntimeErrorKind, SessionId, StructuredOperationRejectionReason,
};
use eam_runtime_gateway::{
    FallbackRuntime, HttpResponsesTransport, InvocationKind, OPENAI_CLOUD_MODEL,
    OPENAI_LOCAL_MODEL, OpenAiResponsesRuntime, ResponsesTransport, RuntimeTarget,
    RuntimeTargetKind, TransportError,
};
use eam_vault::{VaultKey, VaultRepository};
use serde_json::Value;
use tempfile::tempdir;

const CLASSIFICATION_RESPONSE: &str = include_str!("fixtures/classification-response.json");
const TURN_RESPONSE: &str = include_str!("fixtures/turn-response.json");
const UNSUPPORTED_OPERATION_RESPONSE: &str =
    include_str!("fixtures/unsupported-operation-response.json");
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
struct SeenCall {
    target: RuntimeTarget,
    request_json: String,
    timeout: Duration,
}

#[derive(Default)]
struct ScriptedTransport {
    replies: VecDeque<Result<String, TransportError>>,
    seen: Vec<SeenCall>,
}

impl ScriptedTransport {
    fn new(replies: impl IntoIterator<Item = Result<&'static str, TransportError>>) -> Self {
        Self {
            replies: replies
                .into_iter()
                .map(|reply| reply.map(str::to_owned))
                .collect(),
            seen: Vec::new(),
        }
    }

    fn seen(&self) -> &[SeenCall] {
        &self.seen
    }
}

impl ResponsesTransport for ScriptedTransport {
    fn send(
        &mut self,
        target: &RuntimeTarget,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError> {
        self.seen.push(SeenCall {
            target: target.clone(),
            request_json: request_json.to_owned(),
            timeout,
        });
        self.replies
            .pop_front()
            .unwrap_or_else(|| Err(TransportError::other("no scripted response remains")))
    }
}

fn cloud_runtime(
    replies: impl IntoIterator<Item = Result<&'static str, TransportError>>,
) -> OpenAiResponsesRuntime<ScriptedTransport> {
    OpenAiResponsesRuntime::new(
        RuntimeTarget::openai_cloud("https://api.openai.com/v1/responses"),
        ScriptedTransport::new(replies),
        TIMEOUT,
    )
}

fn local_runtime(
    replies: impl IntoIterator<Item = Result<&'static str, TransportError>>,
) -> OpenAiResponsesRuntime<ScriptedTransport> {
    OpenAiResponsesRuntime::new(
        RuntimeTarget::openai_local("http://127.0.0.1:11434/v1/responses"),
        ScriptedTransport::new(replies),
        TIMEOUT,
    )
}

fn serve_one_response(
    response_body: &'static str,
    expected_bearer_token: Option<&'static str>,
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if request.len() >= header_end + content_length {
                break;
            }
        }

        let request_text = String::from_utf8(request).unwrap();
        assert!(request_text.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        let lowercase_request = request_text.to_ascii_lowercase();
        match expected_bearer_token {
            Some(token) => assert!(lowercase_request.contains(&format!(
                "authorization: bearer {}",
                token.to_ascii_lowercase()
            ))),
            None => assert!(!lowercase_request.contains("authorization:")),
        }

        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{address}/v1/responses"), handle)
}

fn run_contract(
    runtime: OpenAiResponsesRuntime<ScriptedTransport>,
) -> (
    eam_core::TurnOutcome,
    Vec<eam_core::Claim>,
    OpenAiResponsesRuntime<ScriptedTransport>,
) {
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    let (selected_id, classification) = core
        .record_person_turn(SessionId::new("source"), "只选择这一条")
        .unwrap();
    assert_eq!(classification, PersonTurnClassification::Question);
    let context = core.freeze_working_context(&[selected_id]).unwrap();
    let outcome = core
        .run_counterpart_turn(SessionId::new("chat"), "请只基于选择内容回答", context)
        .unwrap();
    let claims = core.repository().all_claims().unwrap();
    let (_, runtime, _) = core.into_parts();
    (outcome, claims, runtime)
}

#[test]
fn local_and_cloud_adapters_produce_equivalent_domain_results_from_fixed_fixtures() {
    let replies = [
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(TURN_RESPONSE),
    ];
    let (cloud_outcome, cloud_claims, cloud) = run_contract(cloud_runtime(replies.clone()));
    let (local_outcome, local_claims, local) = run_contract(local_runtime(replies));

    assert_eq!(cloud_outcome, local_outcome);
    assert_eq!(cloud_claims, local_claims);
    assert_eq!(cloud_claims.len(), 1);
    assert_eq!(cloud_claims[0].owner(), ClaimOwner::Counterpart);
    assert_eq!(cloud.disclosures().len(), 3);
    assert_eq!(local.disclosures().len(), 3);
    assert_eq!(
        cloud.disclosures()[2].invocation(),
        InvocationKind::Response
    );
    assert_eq!(
        local.disclosures()[2].invocation(),
        InvocationKind::Response
    );
    assert_eq!(cloud.disclosures()[2].target(), RuntimeTargetKind::Cloud);
    assert_eq!(local.disclosures()[2].target(), RuntimeTargetKind::Local);
    assert_eq!(cloud.disclosures()[2].model(), OPENAI_CLOUD_MODEL);
    assert_eq!(local.disclosures()[2].model(), OPENAI_LOCAL_MODEL);
    assert_eq!(cloud.transport().seen().len(), 3);
    assert_eq!(local.transport().seen().len(), 3);

    for disclosure in [
        cloud.disclosures()[2].request_json(),
        local.disclosures()[2].request_json(),
    ] {
        let request: Value = serde_json::from_str(disclosure).unwrap();
        assert_eq!(request["store"], false);
        assert_eq!(request["reasoning"]["effort"], "low");
        assert_eq!(request["text"]["format"]["strict"], true);
        assert_eq!(request["text"]["format"]["name"], "eam_runtime_response_v1");
        let schema = request["text"]["format"]["schema"].to_string();
        assert!(schema.contains("\"anyOf\""));
        assert!(!schema.contains("\"oneOf\""));
        assert!(!schema.contains("\"const\""));
    }
}

#[test]
fn concrete_http_transport_serves_local_and_rejects_cleartext_cloud_credentials() {
    let (local_endpoint, local_server) = serve_one_response(CLASSIFICATION_RESPONSE, None);
    let local_transport = HttpResponsesTransport::openai_local().unwrap();
    let local_runtime = OpenAiResponsesRuntime::new(
        RuntimeTarget::openai_local(local_endpoint),
        local_transport,
        TIMEOUT,
    );
    let mut local_core = MemoryCore::new(
        InMemoryRepository::new(),
        local_runtime,
        IncrementingClock::new(1_500),
    );
    let (_, local_classification) = local_core
        .record_person_turn(SessionId::new("local-http"), "本地传输")
        .unwrap();
    local_server.join().unwrap();
    assert_eq!(local_classification, PersonTurnClassification::Question);

    assert!(HttpResponsesTransport::openai_cloud("   ").is_err());
    let token = "fixture-cloud-secret";
    let cloud_transport = HttpResponsesTransport::openai_cloud(token).unwrap();
    let cloud_runtime = OpenAiResponsesRuntime::new(
        RuntimeTarget::openai_cloud("http://127.0.0.1:9/v1/responses"),
        cloud_transport,
        TIMEOUT,
    );
    let mut cloud_core = MemoryCore::new(
        InMemoryRepository::new(),
        cloud_runtime,
        IncrementingClock::new(1_600),
    );
    let error = cloud_core
        .record_person_turn(SessionId::new("cloud-http"), "云端传输")
        .expect_err("cloud credentials must never be sent over cleartext HTTP");
    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::Other
    ));
    assert!(
        !cloud_core.runtime().disclosures()[0]
            .request_json()
            .contains(token)
    );
}

#[test]
fn response_payload_contains_only_prompt_and_core_selected_evidence() {
    let replies = [
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(TURN_RESPONSE),
    ];
    let runtime = cloud_runtime(replies);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_000),
    );
    let (selected, _) = core
        .record_person_turn(SessionId::new("source"), "只选择这一条")
        .unwrap();
    core.record_person_turn(SessionId::new("hidden"), "绝不能外发的未选证据")
        .unwrap();
    let context = core.freeze_working_context(&[selected]).unwrap();
    core.run_counterpart_turn(SessionId::new("chat"), "请回答", context)
        .unwrap();

    let request: Value =
        serde_json::from_str(core.runtime().disclosures().last().unwrap().request_json()).unwrap();
    let input = request["input"].as_str().unwrap();
    assert!(input.contains("只选择这一条"));
    assert!(input.contains("请回答"));
    assert!(!input.contains("绝不能外发的未选证据"));
    assert_eq!(
        core.runtime()
            .disclosures()
            .last()
            .unwrap()
            .evidence_ids()
            .iter()
            .map(|id| id.get())
            .collect::<Vec<_>>(),
        [3, 1]
    );
}

#[test]
fn core_rejects_an_operation_outside_the_structured_whitelist() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(UNSUPPORTED_OPERATION_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(3_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(SessionId::new("chat"), "请直接改保险库", context)
        .unwrap();

    assert!(outcome.accepted_judgment_ids().is_empty());
    assert!(outcome.rejected_judgments().is_empty());
    assert_eq!(outcome.rejected_operations().len(), 1);
    assert_eq!(outcome.rejected_operations()[0].operation_index(), 0);
    assert_eq!(
        outcome.rejected_operations()[0].reason(),
        &StructuredOperationRejectionReason::NotWhitelisted("write_vault".to_owned())
    );
    assert!(core.repository().all_claims().unwrap().is_empty());
}

#[test]
fn unavailable_runtime_preserves_already_committed_person_evidence_and_attempt_record() {
    let runtime = cloud_runtime([Err(TransportError::unavailable("network offline"))]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(4_000),
    );

    let error = core
        .record_person_turn(SessionId::new("offline"), "离线时也要保存这句话")
        .expect_err("classification cannot complete while the runtime is unavailable");

    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::Unavailable
    ));
    let evidence = core.repository().all_evidence().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].verbatim(), "离线时也要保存这句话");
    assert!(core.repository().all_claims().unwrap().is_empty());
    assert_eq!(core.runtime().disclosures().len(), 1);
    assert_eq!(core.runtime().transport().seen().len(), 1);
}

#[test]
fn unavailable_runtime_does_not_rollback_encrypted_evidence_across_reopen() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), VaultKey::new([0x62; 32])).unwrap();
    let runtime = cloud_runtime([Err(TransportError::unavailable("network offline"))]);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(4_500));

    core.record_person_turn(SessionId::new("offline"), "重启后仍应存在的原始证据")
        .expect_err("runtime failure should be reported");
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), VaultKey::new([0x62; 32])).unwrap();
    let evidence = repository.all_evidence().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].verbatim(), "重启后仍应存在的原始证据");
    repository.close().unwrap();
}

fn assert_retryable_error_degrades_to_local(error: TransportError) {
    let cloud = cloud_runtime([Err(error)]);
    let local = local_runtime([Ok(CLASSIFICATION_RESPONSE)]);
    let runtime = FallbackRuntime::new(cloud, local);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(5_000),
    );

    let (_, classification) = core
        .record_person_turn(SessionId::new("fallback"), "请在本地继续")
        .unwrap();

    assert_eq!(classification, PersonTurnClassification::Question);
    assert_eq!(core.runtime().primary().disclosures().len(), 1);
    assert_eq!(core.runtime().fallback().disclosures().len(), 1);
    assert_eq!(
        core.runtime().fallback().disclosures()[0].target(),
        RuntimeTargetKind::Local
    );
}

#[test]
fn timeout_and_unavailable_cloud_calls_degrade_to_the_same_local_contract() {
    assert_retryable_error_degrades_to_local(TransportError::timeout("cloud timed out"));
    assert_retryable_error_degrades_to_local(TransportError::unavailable("cloud offline"));
}

#[test]
fn invalid_provider_output_fails_closed_without_fallback() {
    let cloud = cloud_runtime([Ok("{\"output\":[]}")]);
    let local = local_runtime([Ok(CLASSIFICATION_RESPONSE)]);
    let runtime = FallbackRuntime::new(cloud, local);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(6_000),
    );

    let error = core
        .record_person_turn(SessionId::new("invalid"), "结构错误不能换档重试")
        .expect_err("invalid output is not an availability failure");

    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::InvalidResponse
    ));
    assert_eq!(core.runtime().primary().disclosures().len(), 1);
    assert!(core.runtime().fallback().disclosures().is_empty());
}
