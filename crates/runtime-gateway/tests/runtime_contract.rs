use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::Duration,
};

use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, CoreError, DecisionImpact, DisputeState,
    EvidenceCitation, EvidenceId, FrozenEvidenceBlock, FrozenMemoryDispute, FrozenRetrievalWindow,
    InMemoryRepository, IncrementingClock, MemoryCore, MemoryRepository, PersonTurnClassification,
    RetrievalSnapshot, RetrievedContextItem, RuntimeErrorKind, SessionId,
    SharedAgreementCandidateStatus, SharedAgreementRevision, SharedExperienceRepository,
    SourceCurrentness, StructuredOperationRejectionReason, Timestamp, Uncertainty,
};
use eam_runtime_gateway::{
    FallbackRuntime, HttpResponsesTransport, InvocationKind, OPENAI_CLOUD_MODEL,
    OPENAI_LOCAL_MODEL, OpenAiResponsesRuntime, OutboundContextSource, ResponsesTransport,
    RuntimeTarget, RuntimeTargetKind, TransportError,
};
use eam_vault::{VaultKey, VaultRepository};
use serde_json::Value;
use tempfile::tempdir;

const CLASSIFICATION_RESPONSE: &str = include_str!("fixtures/classification-response.json");
const TURN_RESPONSE: &str = include_str!("fixtures/turn-response.json");
const UNSUPPORTED_OPERATION_RESPONSE: &str =
    include_str!("fixtures/unsupported-operation-response.json");
const SHARED_EXPERIENCE_RESPONSE: &str = include_str!("fixtures/shared-experience-response.json");
const SHARED_AGREEMENT_RESPONSE: &str = include_str!("fixtures/shared-agreement-response.json");
const SHARED_AGREEMENT_ASSENT_RESPONSE: &str =
    include_str!("fixtures/shared-agreement-assent-response.json");
const HIGH_IMPACT_DISPUTE_RESPONSE: &str =
    include_str!("fixtures/high-impact-dispute-response.json");
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
fn response_payload_and_disclosure_contain_only_the_frozen_retrieval_result() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(TURN_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_500),
    );
    let (selected, _) = core
        .record_person_turn(SessionId::new("source"), "只选择这一条")
        .unwrap();
    core.record_person_turn(SessionId::new("hidden"), "绝不能外发的向量原始候选")
        .unwrap();
    let context = core
        .freeze_working_context(&[selected])
        .unwrap()
        .with_retrieval(
            vec![RetrievedContextItem::EvidenceWindow(
                FrozenRetrievalWindow::new(
                    0,
                    vec![FrozenEvidenceBlock::new(
                        900,
                        901,
                        0,
                        "只外发冻结后的权威证据块".to_owned(),
                        88,
                        "notes/frozen.md".to_owned(),
                        SourceCurrentness::Present,
                        Timestamp::from_millis(2_400),
                    )],
                    40,
                ),
            )],
            RetrievalSnapshot::new("eam-retrieval-v2", "model-v1", 128, 40, [9; 32]),
        )
        .unwrap();
    core.run_counterpart_turn(SessionId::new("chat"), "请回答", context)
        .unwrap();

    let disclosure = core.runtime().disclosures().last().unwrap();
    let request: Value = serde_json::from_str(disclosure.request_json()).unwrap();
    let input = request["input"].as_str().unwrap();
    assert!(input.contains("只外发冻结后的权威证据块"));
    assert!(input.contains("notes/frozen.md"));
    assert!(input.contains("eam-retrieval-v2"));
    assert!(!input.contains("绝不能外发的向量原始候选"));
    assert!(!input.contains("embedding"));
    assert_eq!(
        disclosure.retrieved_sources(),
        [OutboundContextSource::EvidenceBlock {
            evidence_id: 900,
            block_id: 901,
        }]
    );
}

#[test]
fn ordinary_dispute_context_preserves_the_pair_without_a_fixed_disclosure_template() {
    let runtime = run_disputed_contract(DecisionImpact::Ordinary, TURN_RESPONSE).unwrap();
    let disclosure = runtime.disclosures().last().unwrap();
    let request: Value = serde_json::from_str(disclosure.request_json()).unwrap();
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("do not narrate internal state names"));
    assert!(instructions.contains("do not") && instructions.contains("fixed disclosure template"));

    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["working_context"]["decision_impact"], "ordinary");
    assert_eq!(
        input["working_context"]["disclosure_policy"],
        "natural_material_disagreement"
    );
    let pair = &input["working_context"]["retrieved"][0];
    assert_eq!(pair["kind"], "memory_dispute");
    assert_eq!(pair["counterpart_view"], "Planning has become steadier");
    assert_eq!(
        pair["person_position"],
        "One exceptional week should not define the pattern"
    );
    assert_eq!(pair["state"], "maintained");
    assert_eq!(pair["counterpart_sources"].as_array().unwrap().len(), 1);
    assert_eq!(pair["person_evidence"].as_array().unwrap().len(), 1);
    assert!(
        disclosure
            .retrieved_sources()
            .contains(&OutboundContextSource::MemoryDispute {
                memory_id: 41,
                dispute_id: 51,
            })
    );
    assert!(
        disclosure
            .retrieved_sources()
            .contains(&OutboundContextSource::LedgerClaim {
                claim_id: ClaimId::from_raw(61),
            })
    );
}

#[test]
fn high_impact_dispute_requires_proactive_uncertainty_with_an_evidence_entry() {
    let runtime =
        run_disputed_contract(DecisionImpact::High, HIGH_IMPACT_DISPUTE_RESPONSE).unwrap();
    let request: Value =
        serde_json::from_str(runtime.disclosures().last().unwrap().request_json()).unwrap();
    assert!(
        request["instructions"]
            .as_str()
            .unwrap()
            .contains("naturally and proactively explain material uncertainty")
    );
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["working_context"]["decision_impact"], "high");
    assert_eq!(
        input["working_context"]["disclosure_policy"],
        "proactive_uncertainty_with_evidence_entry"
    );

    let Err(error) = run_disputed_contract(DecisionImpact::High, UNSUPPORTED_OPERATION_RESPONSE)
    else {
        panic!("high-impact disputed output without a cited entry point must fail closed");
    };
    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::InvalidResponse
    ));
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
fn shared_experience_operation_is_whitelisted_and_keeps_typed_evidence() {
    let runtime = cloud_runtime([Ok(CLASSIFICATION_RESPONSE), Ok(SHARED_EXPERIENCE_RESPONSE)]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();

    let outcome = core
        .run_counterpart_turn(SessionId::new("chat"), "这件事无关紧要。", context)
        .unwrap();

    assert_eq!(outcome.admitted_shared_experience_ids().len(), 1);
    assert!(outcome.pending_agreement_candidate_ids().is_empty());
    assert!(outcome.rejected_shared_experiences().is_empty());
    assert!(outcome.rejected_operations().is_empty());
    let shared = core
        .repository()
        .all_claims()
        .unwrap()
        .into_iter()
        .find(|claim| claim.owner() == ClaimOwner::Shared)
        .unwrap();
    assert_eq!(shared.support().len(), 2);

    let request: Value =
        serde_json::from_str(core.runtime().disclosures().last().unwrap().request_json()).unwrap();
    let operations = &request["text"]["format"]["schema"]["properties"]["operations"];
    assert!(operations.to_string().contains("propose_shared_experience"));
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("substantive disagreement with incompatible"));
    assert!(
        instructions.contains("if removing the digital counterpart leaves the event fully intact")
    );
}

#[test]
fn agreement_boundaries_and_pending_exact_version_are_in_the_runtime_contract() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_ASSENT_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("chat"),
            "我同意复盘时直接指出关键逃避。",
            context,
        )
        .unwrap();
    let first_id = first.pending_agreement_candidate_ids()[0];
    let first_candidate = core
        .repository()
        .shared_agreement_candidate(first_id)
        .unwrap()
        .unwrap();
    assert_eq!(first_candidate.scope(), Some("双方共同项目复盘"));
    assert_eq!(
        first_candidate.effective_from(),
        Some(Timestamp::from_millis(2_000))
    );

    let second_id = core
        .revise_shared_agreement(
            first_id,
            SessionId::new("chat"),
            SharedAgreementRevision::new(
                "只在正式复盘中直接指出关键逃避",
                "双方共同项目的正式复盘",
                Timestamp::from_millis(3_000),
                None,
                None,
            ),
        )
        .unwrap();
    let context = core.freeze_working_context(&[]).unwrap();
    let second = core
        .run_counterpart_turn(SessionId::new("chat"), "请核对第二版。", context)
        .unwrap();
    assert_eq!(second.assented_agreement_candidate_ids(), &[second_id]);
    assert_eq!(
        core.repository()
            .shared_agreement_candidate(second_id)
            .unwrap()
            .unwrap()
            .status(),
        SharedAgreementCandidateStatus::AwaitingPerson
    );

    let response_disclosures = core
        .runtime()
        .disclosures()
        .iter()
        .filter(|record| record.invocation() == InvocationKind::Response)
        .collect::<Vec<_>>();
    let assent_request: Value =
        serde_json::from_str(response_disclosures[1].request_json()).unwrap();
    let input: Value = serde_json::from_str(assent_request["input"].as_str().unwrap()).unwrap();
    let pending = &input["pending_agreement_candidates"][0];
    assert_eq!(pending["candidate_id"], second_id.get());
    assert_eq!(pending["version"], 2);
    assert_eq!(pending["scope"], "双方共同项目的正式复盘");
    assert_eq!(pending["effective_from_millis"], 3_000);
    assert!(
        assent_request["text"]["format"]["schema"]
            .to_string()
            .contains("assent_shared_agreement_candidate")
    );
    assert!(
        response_disclosures[1]
            .evidence_ids()
            .contains(&EvidenceId::from_raw(3))
    );
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

fn run_disputed_contract(
    impact: DecisionImpact,
    response: &'static str,
) -> Result<OpenAiResponsesRuntime<ScriptedTransport>, CoreError> {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(response),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(7_000),
    );
    let (selected, _) = core
        .record_person_turn(SessionId::new("dispute-source"), "只选择这一条")
        .unwrap();
    let source = Claim::restore(
        ClaimId::from_raw(61),
        ClaimOwner::Counterpart,
        "Planning has become steadier".to_owned(),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "只选择这一条",
        )],
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(10)),
        Timestamp::from_millis(20),
    );
    let dispute = FrozenMemoryDispute::new(
        51,
        41,
        1,
        "Planning has become steadier".to_owned(),
        vec![source],
        "One exceptional week should not define the pattern".to_owned(),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "只选择这一条",
        )],
        Some("The longer sequence remains persuasive".to_owned()),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "只选择这一条",
        )],
        DisputeState::Maintained,
        96,
    );
    let context = core
        .freeze_working_context(&[selected])
        .unwrap()
        .with_retrieval(
            vec![RetrievedContextItem::MemoryDispute(dispute)],
            RetrievalSnapshot::new("eam-retrieval-v2", "model-v1", 128, 96, [7; 32]),
        )
        .unwrap()
        .with_decision_impact(impact);
    core.run_counterpart_turn(SessionId::new("chat"), "请回答", context)?;
    let (_, runtime, _) = core.into_parts();
    Ok(runtime)
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
