use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread::{self, JoinHandle},
    time::Duration,
};

use eam_core::{
    ActiveRelationalConstraint, ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence,
    CoreError, DecisionImpact, DisputeState, EvidenceCitation, EvidenceId, FrozenEvidenceBlock,
    FrozenMemoryDispute, FrozenRetrievalWindow, IdentityEvolutionRepository,
    IdentityProfileSnapshot, IdentityRuntimeContext, IdentityStateSnapshot, InMemoryRepository,
    IncrementingClock, MemoryCore, MemoryRepository, PatternMaturityWriteRejectionReason,
    PersonTurnClassification, ReflectionImportance, ReflectionInvitation,
    ReflectionInvitationBasis, ReflectionInvitationRepository, ReflectionInvitationState,
    ReflectionOpportunity, RetrievalSnapshot, RetrievedContextItem, RuntimeErrorKind, SessionId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementRevision,
    SharedExperienceKind, SharedExperienceRepository, SourceCurrentness, Speaker,
    StructuredOperationRejectionReason, Timestamp, Uncertainty, WorkingContext,
};
use eam_memory::{
    LongTermMemoryRepository, MemoryBasis, MemoryConfidence, MemoryId, MemoryKind,
    MemoryMaintenance, MemoryProposal, MemoryStatus, MemorySubject,
};
use eam_runtime_gateway::{
    FallbackRuntime, HttpResponsesTransport, InvocationKind, OpenAiResponsesRuntime,
    OutboundContextSource, ResponsesTransport, RuntimeTarget, RuntimeTargetKind, TransportError,
    TransportErrorKind,
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
const SHARED_AGREEMENT_SUPERSESSION_RESPONSE: &str =
    include_str!("fixtures/shared-agreement-supersession-response.json");
const SHARED_AGREEMENT_ASSENT_RESPONSE: &str =
    include_str!("fixtures/shared-agreement-assent-response.json");
const RELATIONAL_CONSTRAINT_DEPARTURE_RESPONSE: &str =
    include_str!("fixtures/relational-constraint-departure-response.json");
const AGREEMENT_WITHDRAWAL_RESPONSE: &str =
    include_str!("fixtures/agreement-withdrawal-response.json");
const AGREEMENT_WITHDRAWAL_MISSING_REASON_RESPONSE: &str =
    include_str!("fixtures/agreement-withdrawal-missing-reason-response.json");
const IDENTITY_REVISION_RESPONSE: &str = include_str!("fixtures/identity-revision-response.json");
const REFLECTION_INVITATION_RESPONSE: &str =
    include_str!("fixtures/reflection-invitation-response.json");
const PATTERN_MATURITY_RESPONSE: &str = include_str!("fixtures/pattern-maturity-response.json");
const PATTERN_MATURITY_DUPLICATE_RESPONSE: &str =
    include_str!("fixtures/pattern-maturity-duplicate-response.json");
const PATTERN_MATURITY_INELIGIBLE_RESPONSE: &str =
    include_str!("fixtures/pattern-maturity-ineligible-response.json");
const PATTERN_MATURITY_UNKNOWN_RESPONSE: &str =
    include_str!("fixtures/pattern-maturity-unknown-response.json");
const PATTERN_MATURITY_MALFORMED_RESPONSE: &str =
    include_str!("fixtures/pattern-maturity-malformed-response.json");
const HIGH_IMPACT_DISPUTE_RESPONSE: &str =
    include_str!("fixtures/high-impact-dispute-response.json");
const TIMEOUT: Duration = Duration::from_secs(30);
const CLOUD_MODEL: &str = "gpt-5.6-terra";
const LOCAL_MODEL: &str = "gpt-oss-20b";

#[derive(Clone, Debug, PartialEq, Eq)]
struct SeenCall {
    responses_endpoint: String,
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
        _target: &RuntimeTarget,
        responses_endpoint: &str,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError> {
        self.seen.push(SeenCall {
            responses_endpoint: responses_endpoint.to_owned(),
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
        RuntimeTarget::new("https://api.openai.com/v1", CLOUD_MODEL).unwrap(),
        ScriptedTransport::new(replies),
        TIMEOUT,
    )
}

fn local_runtime(
    replies: impl IntoIterator<Item = Result<&'static str, TransportError>>,
) -> OpenAiResponsesRuntime<ScriptedTransport> {
    OpenAiResponsesRuntime::new(
        RuntimeTarget::new("http://127.0.0.1:11434/v1", LOCAL_MODEL).unwrap(),
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
    (format!("http://{address}/v1"), handle)
}

fn serve_one_redirect() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("POST /v1/responses HTTP/1.1\r\n"));
        write!(
            stream,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/must-not-follow\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{address}/v1"), handle)
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
    assert_eq!(cloud.disclosures()[2].model(), CLOUD_MODEL);
    assert_eq!(local.disclosures()[2].model(), LOCAL_MODEL);
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
fn concrete_http_transport_appends_responses_and_keeps_optional_bearer_out_of_records() {
    let (local_endpoint, local_server) = serve_one_response(CLASSIFICATION_RESPONSE, None);
    let local_transport = HttpResponsesTransport::new(None).unwrap();
    let local_runtime = OpenAiResponsesRuntime::new(
        RuntimeTarget::new(local_endpoint, LOCAL_MODEL).unwrap(),
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

    assert!(HttpResponsesTransport::new(Some("   ".to_owned())).is_err());
    let token = "synthetic-bearer-secret";
    let (keyed_endpoint, keyed_server) = serve_one_response(CLASSIFICATION_RESPONSE, Some(token));
    let keyed_transport = HttpResponsesTransport::new(Some(token.to_owned())).unwrap();
    let keyed_runtime = OpenAiResponsesRuntime::new(
        RuntimeTarget::new(keyed_endpoint, "custom-model-id").unwrap(),
        keyed_transport,
        TIMEOUT,
    );
    let mut keyed_core = MemoryCore::new(
        InMemoryRepository::new(),
        keyed_runtime,
        IncrementingClock::new(1_600),
    );
    let (_, classification) = keyed_core
        .record_person_turn(SessionId::new("keyed-loopback"), "带合成密钥的本地传输")
        .unwrap();
    keyed_server.join().unwrap();
    assert_eq!(classification, PersonTurnClassification::Question);
    assert!(
        !keyed_core.runtime().disclosures()[0]
            .request_json()
            .contains(token)
    );
    assert_eq!(
        keyed_core.runtime().disclosures()[0].model(),
        "custom-model-id"
    );
}

#[test]
fn runtime_target_accepts_remote_https_and_loopback_http_only() {
    let accepted = [
        ("https://api.example.test/v1/", RuntimeTargetKind::Cloud),
        ("https://192.0.2.10/runtime", RuntimeTargetKind::Cloud),
        ("http://127.0.0.1:11434/v1/", RuntimeTargetKind::Local),
        ("http://localhost:11434/v1", RuntimeTargetKind::Local),
        ("http://[::1]:11434/v1", RuntimeTargetKind::Local),
    ];
    for (base_url, expected_kind) in accepted {
        let target = RuntimeTarget::new(base_url, "custom-model").unwrap();
        assert_eq!(target.kind(), expected_kind);
        assert!(target.responses_endpoint().ends_with("/responses"));
        assert!(!target.responses_endpoint().contains("//responses"));
    }

    for base_url in [
        "http://api.example.test/v1",
        "ftp://localhost/v1",
        "https://user:password@example.test/v1",
        "https://example.test/v1?tenant=one",
        "https://example.test/v1#fragment",
        "https://example.test/v1 responses",
        "not-a-url",
    ] {
        assert!(
            RuntimeTarget::new(base_url, "custom-model").is_err(),
            "accepted invalid Base URL: {base_url}"
        );
    }
    assert!(RuntimeTarget::new("https://example.test/v1", "   ").is_err());
}

#[test]
fn custom_model_is_used_in_the_request_and_outbound_disclosure() {
    let custom_model = "owner/model-with-custom-revision";
    let runtime = OpenAiResponsesRuntime::new(
        RuntimeTarget::new("https://runtime.example.test/openai/v1/", custom_model).unwrap(),
        ScriptedTransport::new([Ok(CLASSIFICATION_RESPONSE)]),
        TIMEOUT,
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_700),
    );
    let (_, classification) = core
        .record_person_turn(SessionId::new("custom-model"), "只验证模型透传")
        .unwrap();
    assert_eq!(classification, PersonTurnClassification::Question);
    let request: Value =
        serde_json::from_str(core.runtime().disclosures()[0].request_json()).unwrap();
    assert_eq!(request["model"], custom_model);
    assert_eq!(core.runtime().disclosures()[0].model(), custom_model);
    assert_eq!(
        core.runtime().transport().seen()[0].responses_endpoint,
        "https://runtime.example.test/openai/v1/responses"
    );
}

#[test]
fn redirect_is_rejected_without_exposing_the_bearer_secret() {
    let token = "synthetic-redirect-secret";
    let (base_url, server) = serve_one_redirect();
    let target = RuntimeTarget::new(base_url, "redirect-test-model").unwrap();
    let mut transport = HttpResponsesTransport::new(Some(token.to_owned())).unwrap();
    let responses_endpoint = target.responses_endpoint();
    let error = transport
        .send(&target, &responses_endpoint, "{}", TIMEOUT)
        .expect_err("3xx responses must not be followed or accepted");
    server.join().unwrap();
    assert_eq!(error.kind(), TransportErrorKind::Other);
    assert!(error.to_string().contains("307"));
    assert!(!error.to_string().contains(token));
}

#[test]
fn concrete_transport_rejects_an_endpoint_not_derived_from_the_validated_target() {
    let target = RuntimeTarget::new("http://127.0.0.1:11434/v1", "safe-model").unwrap();
    let mut transport = HttpResponsesTransport::new(Some("synthetic-secret".to_owned())).unwrap();
    let error = transport
        .send(
            &target,
            "http://remote.example.test/v1/responses",
            "{}",
            TIMEOUT,
        )
        .expect_err("transport must not accept an unvalidated endpoint override");
    assert_eq!(error.kind(), TransportErrorKind::Other);
    assert_eq!(
        error.to_string(),
        "Responses endpoint does not match the validated target"
    );
    assert!(!error.to_string().contains("synthetic-secret"));
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
fn runtime_receives_the_current_self_bundle_identity_and_emits_a_strict_revision() {
    let identity = IdentityStateSnapshot::restore(
        1,
        None,
        IdentityProfileSnapshot::new(
            "岚",
            "温和、克制",
            "保留分歧",
            "准确高于迎合",
            "同行者",
            "帮助本人看见长期变化",
        ),
        "基于初始自我介绍形成",
        Vec::new(),
        Timestamp::from_millis(10),
    );
    let repository = InMemoryRepository::new()
        .with_identity_context(IdentityRuntimeContext::new(7, 1, identity))
        .unwrap();
    let runtime = cloud_runtime([Ok(CLASSIFICATION_RESPONSE), Ok(IDENTITY_REVISION_RESPONSE)]);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(2_500));
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("identity-runtime"),
            "最近我更需要直白但不武断的提醒。",
            context,
        )
        .unwrap();

    assert_eq!(
        outcome
            .accepted_identity_revision()
            .unwrap()
            .identity_version(),
        2
    );
    assert_eq!(core.repository().identity_history().unwrap().len(), 2);
    let disclosure = core.runtime().disclosures().last().unwrap();
    assert!(
        disclosure
            .retrieved_sources()
            .contains(&OutboundContextSource::IdentityState { version: 1 })
    );
    let request: Value = serde_json::from_str(disclosure.request_json()).unwrap();
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["identity"]["constitution_version"], 7);
    assert_eq!(input["identity"]["self_bundle_version"], 1);
    assert_eq!(input["identity"]["state"]["version"], 1);
    assert_eq!(input["identity"]["state"]["name"], "岚");
    assert!(
        request["text"]["format"]["schema"]
            .to_string()
            .contains("propose_identity_revision")
    );
}

#[test]
fn runtime_receives_one_scheduled_reflection_and_emits_a_strict_sourced_invitation() {
    let mut repository = InMemoryRepository::new();
    let source_id = repository.next_evidence_id();
    repository
        .append_evidence(ConversationEvidence::restore(
            source_id,
            SessionId::new("reflection-source"),
            Speaker::Person,
            "工作再次挤压了真实生活。".to_owned(),
            Timestamp::from_millis(10),
        ))
        .unwrap();
    let invitation = ReflectionInvitation::restore(
        repository.next_reflection_invitation_id(),
        "工作挤压生活",
        "你刚才明确说工作再次挤压了真实生活。",
        vec![EvidenceCitation::new(source_id, "工作再次挤压了真实生活。")],
        "这是一项有直接证据的重要变化。",
        ReflectionImportance::Important,
        ReflectionInvitationBasis::ImportantSingleChange,
        ReflectionInvitationState::Pending,
        Timestamp::from_millis(20),
        Timestamp::from_millis(20),
        None,
        None,
        0,
        false,
    );
    repository
        .commit_reflection_invitation(invitation.clone())
        .unwrap();
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(REFLECTION_INVITATION_RESPONSE),
    ]);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(2_700));
    let context = core
        .freeze_working_context(&[source_id])
        .unwrap()
        .with_reflection_opportunity(ReflectionOpportunity::RelatedTopic(
            "工作挤压生活".to_owned(),
        ));
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("reflection-runtime"),
            "我想继续聊聊这项变化。",
            context,
        )
        .unwrap();

    assert_eq!(
        outcome.offered_reflection_invitation_id(),
        Some(invitation.id())
    );
    assert_eq!(outcome.accepted_reflection_invitations().len(), 1);
    let invitations = core.repository().all_reflection_invitations().unwrap();
    assert_eq!(invitations.len(), 2);
    assert_eq!(invitations[0].state(), ReflectionInvitationState::Offered);
    assert_eq!(invitations[1].topic_key(), "新的重要变化");

    let disclosure = core.runtime().disclosures().last().unwrap();
    assert!(disclosure.retrieved_sources().contains(
        &OutboundContextSource::ReflectionInvitation {
            invitation_id: invitation.id().get(),
        }
    ));
    assert_eq!(
        disclosure
            .evidence_ids()
            .iter()
            .map(|id| id.get())
            .collect::<Vec<_>>(),
        [2, 1]
    );
    let request: Value = serde_json::from_str(disclosure.request_json()).unwrap();
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["reflection"]["disposition"], "offer");
    assert_eq!(
        input["reflection"]["invitation"]["id"],
        invitation.id().get()
    );
    assert_eq!(
        input["reflection"]["invitation"]["topic_key"],
        "工作挤压生活"
    );
    assert_eq!(input["reflection"]["invitation"]["state"], "pending");
    assert_eq!(input["reflection"]["invitation"]["defer_count"], 0);
    assert_eq!(input["reflection"]["invitation"]["mute_prompted"], false);
    assert!(
        request["text"]["format"]["schema"]
            .to_string()
            .contains("propose_reflection_invitation")
    );
}

#[test]
fn runtime_strict_pattern_maturity_schema_rejects_an_incomplete_operation() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(PATTERN_MATURITY_MALFORMED_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_800),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let error = core
        .run_counterpart_turn(
            SessionId::new("pattern-maturity-malformed"),
            "Please review the pattern.",
            context,
        )
        .expect_err("missing rationale must fail the runtime contract");
    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::InvalidResponse
    ));
}

#[test]
fn runtime_unknown_and_ineligible_pattern_maturity_are_rejected_without_writing() {
    for response in [
        PATTERN_MATURITY_UNKNOWN_RESPONSE,
        PATTERN_MATURITY_INELIGIBLE_RESPONSE,
    ] {
        let directory = tempdir().unwrap();
        let (outcome, core) = run_seeded_pattern_contract(directory.path(), response);

        assert!(outcome.accepted_pattern_maturities().is_empty());
        assert_eq!(outcome.rejected_pattern_maturities().len(), 1);
        assert_eq!(
            outcome.rejected_pattern_maturities()[0].reason(),
            &PatternMaturityWriteRejectionReason::QualificationRejected
        );
        let pattern_id = MemoryId::new(1).unwrap();
        let current = core
            .repository()
            .current_memory(pattern_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.version(), 1);
        assert_eq!(current.status(), MemoryStatus::ProvisionalPattern);
        assert!(
            core.repository()
                .pattern_maturity_records(pattern_id)
                .unwrap()
                .is_empty()
        );
        let (repository, _, _) = core.into_parts();
        repository.close().unwrap();
    }
}

#[test]
fn runtime_pattern_maturity_uses_memory_qualification_and_commits_the_successor() {
    let directory = tempdir().unwrap();
    let (outcome, core) = run_seeded_pattern_contract(directory.path(), PATTERN_MATURITY_RESPONSE);

    assert!(outcome.rejected_operations().is_empty());
    assert!(outcome.rejected_pattern_maturities().is_empty());
    assert_eq!(outcome.accepted_pattern_maturities().len(), 1);
    assert_eq!(outcome.accepted_pattern_maturities()[0].memory_id(), 1);
    assert_eq!(outcome.accepted_pattern_maturities()[0].memory_version(), 2);
    let pattern_id = MemoryId::new(1).unwrap();
    let current = core
        .repository()
        .current_memory(pattern_id)
        .unwrap()
        .unwrap();
    assert_eq!(current.version(), 2);
    assert_eq!(current.status(), MemoryStatus::SupportedCounterpartView);
    assert_eq!(
        core.repository()
            .pattern_maturity_records(pattern_id)
            .unwrap()
            .len(),
        1
    );

    let request: Value =
        serde_json::from_str(core.runtime().disclosures().last().unwrap().request_json()).unwrap();
    let schema = request["text"]["format"]["schema"].to_string();
    assert!(schema.contains("propose_pattern_maturity"));
    assert!(schema.contains("counterexample_review_ref"));
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("Qualification never auto-upgrades"));
    assert!(instructions.contains("person discussion is not approval"));
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
}

#[test]
fn runtime_rejects_a_duplicate_pattern_maturity_after_one_commit() {
    let directory = tempdir().unwrap();
    let (outcome, core) =
        run_seeded_pattern_contract(directory.path(), PATTERN_MATURITY_DUPLICATE_RESPONSE);

    assert_eq!(outcome.accepted_pattern_maturities().len(), 1);
    assert_eq!(outcome.rejected_pattern_maturities().len(), 1);
    assert_eq!(
        outcome.rejected_pattern_maturities()[0].reason(),
        &PatternMaturityWriteRejectionReason::DuplicateProposal
    );
    let pattern_id = MemoryId::new(1).unwrap();
    assert_eq!(
        core.repository()
            .pattern_maturity_records(pattern_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        core.repository().memory_versions(pattern_id).unwrap().len(),
        2
    );
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
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
    assert_eq!(pending["supersedes_agreement_ids"], serde_json::json!([]));
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
fn explicit_whole_supersession_is_in_the_strict_runtime_contract() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_SUPERSESSION_RESPONSE),
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
    let original_claim_id = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let context = WorkingContext::from_selected_evidence(Vec::new(), Timestamp::from_millis(3_000))
        .with_active_relational_constraints(vec![
            ActiveRelationalConstraint::new(
                original_claim_id,
                "复盘中直接指出关键逃避",
                "双方共同项目复盘",
                Timestamp::from_millis(2_000),
                None,
            )
            .unwrap(),
        ])
        .unwrap();
    let second = core
        .run_counterpart_turn(
            SessionId::new("chat"),
            "我同意新约定整份取代旧约定。",
            context,
        )
        .unwrap();

    assert!(second.rejected_shared_experiences().is_empty());
    let replacement = core
        .repository()
        .shared_agreement_candidate(second.pending_agreement_candidate_ids()[0])
        .unwrap()
        .unwrap();
    assert_eq!(replacement.supersedes_agreement_ids(), &[original_claim_id]);
    let disclosure = core
        .runtime()
        .disclosures()
        .iter()
        .filter(|record| record.invocation() == InvocationKind::Response)
        .nth(1)
        .unwrap();
    let request: Value = serde_json::from_str(disclosure.request_json()).unwrap();
    let schema = request["text"]["format"]["schema"].to_string();
    assert!(schema.contains("supersedes_agreement_ids"));
    assert!(
        request["instructions"]
            .as_str()
            .unwrap()
            .contains("every entire displaced agreement Claim ID")
    );
    assert!(
        disclosure
            .retrieved_sources()
            .contains(&OutboundContextSource::LedgerClaim {
                claim_id: original_claim_id,
            })
    );
}

#[test]
fn active_constraint_and_reasoned_departure_share_the_strict_runtime_contract() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(RELATIONAL_CONSTRAINT_DEPARTURE_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    let first_context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("chat"),
            "我同意复盘时直接指出关键逃避。",
            first_context,
        )
        .unwrap();
    let agreement_claim_id = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let constraint = ActiveRelationalConstraint::new(
        agreement_claim_id,
        "复盘时直接指出关键逃避",
        "双方共同项目复盘",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();
    let context = WorkingContext::from_selected_evidence(Vec::new(), Timestamp::from_millis(3_000))
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    let second = core
        .run_counterpart_turn(SessionId::new("chat"), "请替我执行现实操作", context)
        .unwrap();

    assert_eq!(second.recorded_constraint_departure_ids().len(), 1);
    assert_eq!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .iter()
            .filter(|experience| experience.kind() == SharedExperienceKind::AgreementBreach)
            .count(),
        1
    );
    let response_disclosure = core
        .runtime()
        .disclosures()
        .iter()
        .filter(|record| record.invocation() == InvocationKind::Response)
        .nth(1)
        .unwrap();
    let request: Value = serde_json::from_str(response_disclosure.request_json()).unwrap();
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("always below the constitution, safety boundaries"));
    assert!(instructions.contains("depart_relational_constraint"));
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    let projected = &input["working_context"]["active_relational_constraints"][0];
    assert_eq!(projected["agreement_claim_id"], agreement_claim_id.get());
    assert_eq!(projected["scope"], "双方共同项目复盘");
    assert_eq!(
        projected["priority"],
        "below_constitution_safety_and_action_authorization"
    );
    assert!(
        request["text"]["format"]["schema"]
            .to_string()
            .contains("depart_relational_constraint")
    );
    assert!(response_disclosure.retrieved_sources().contains(
        &OutboundContextSource::LedgerClaim {
            claim_id: agreement_claim_id,
        }
    ));
}

#[test]
fn counterpart_withdrawal_is_distinct_immediate_and_non_vetoable() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(SHARED_AGREEMENT_RESPONSE),
        Ok(CLASSIFICATION_RESPONSE),
        Ok(AGREEMENT_WITHDRAWAL_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_000),
    );
    let first_context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("chat"),
            "我同意复盘时直接指出关键逃避。",
            first_context,
        )
        .unwrap();
    let agreement_claim_id = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let constraint = ActiveRelationalConstraint::new(
        agreement_claim_id,
        "复盘时直接指出关键逃避",
        "双方共同项目复盘",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();
    let context = WorkingContext::from_selected_evidence(Vec::new(), Timestamp::from_millis(3_000))
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    let second = core
        .run_counterpart_turn(SessionId::new("chat"), "你仍愿意遵守吗？", context)
        .unwrap();

    assert_eq!(second.recorded_agreement_withdrawal_ids().len(), 1);
    assert!(second.recorded_constraint_departure_ids().is_empty());
    let experiences = core.repository().all_shared_experiences().unwrap();
    let withdrawal = experiences
        .iter()
        .find_map(eam_core::SharedExperience::agreement_withdrawal)
        .unwrap();
    assert_eq!(withdrawal.agreement_claim_id(), agreement_claim_id);
    assert_eq!(withdrawal.reason(), Some("它已妨碍我诚实表达独立判断"));
    let response_disclosure = core
        .runtime()
        .disclosures()
        .iter()
        .filter(|record| record.invocation() == InvocationKind::Response)
        .nth(1)
        .unwrap();
    let request: Value = serde_json::from_str(response_disclosure.request_json()).unwrap();
    let instructions = request["instructions"].as_str().unwrap();
    assert!(instructions.contains("withdraw_shared_agreement"));
    assert!(instructions.contains("person approval is forbidden"));
    let schema = request["text"]["format"]["schema"].to_string();
    assert!(schema.contains("withdraw_shared_agreement"));
    assert!(schema.contains("depart_relational_constraint"));
}

#[test]
fn withdrawal_missing_required_reason_fails_the_strict_runtime_contract() {
    let runtime = cloud_runtime([
        Ok(CLASSIFICATION_RESPONSE),
        Ok(AGREEMENT_WITHDRAWAL_MISSING_REASON_RESPONSE),
    ]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(4_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();

    let error = core
        .run_counterpart_turn(SessionId::new("chat"), "请退出这项约定。", context)
        .expect_err("withdrawal without a reason must fail closed");

    assert!(matches!(
        error,
        CoreError::Runtime(ref runtime_error)
            if runtime_error.kind() == RuntimeErrorKind::InvalidResponse
    ));
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

fn run_seeded_pattern_contract(
    directory: &Path,
    turn_response: &'static str,
) -> (
    eam_core::TurnOutcome,
    MemoryCore<VaultRepository, OpenAiResponsesRuntime<ScriptedTransport>, IncrementingClock>,
) {
    let repository = seed_pattern_vault(directory);
    let runtime = cloud_runtime([Ok(CLASSIFICATION_RESPONSE), Ok(turn_response)]);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(2_000));
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("pattern-maturity-runtime"),
            "Please decide whether the reviewed pattern should remain provisional.",
            context,
        )
        .unwrap();
    (outcome, core)
}

fn seed_pattern_vault(directory: &Path) -> VaultRepository {
    let mut repository = VaultRepository::open(directory, VaultKey::new([0x72; 32])).unwrap();
    for evidence in [
        pattern_conversation(
            1,
            Speaker::Person,
            "I reviewed plans calmly in January",
            100,
        ),
        pattern_conversation(
            2,
            Speaker::Person,
            "I reviewed plans calmly in February",
            200,
        ),
        pattern_conversation(3, Speaker::Person, "I reviewed plans calmly in March", 300),
        pattern_conversation(
            4,
            Speaker::Person,
            "I reviewed plans calmly in April",
            1_200,
        ),
        pattern_conversation(
            10,
            Speaker::Counterpart,
            "I checked the initial sequence for exceptions",
            350,
        ),
        pattern_conversation(
            11,
            Speaker::Counterpart,
            "I checked the newer sequence for exceptions",
            1_300,
        ),
        pattern_conversation(
            12,
            Speaker::Person,
            "I think that pattern fits some weeks, but not every week",
            1_400,
        ),
        pattern_conversation(
            13,
            Speaker::Counterpart,
            "I agree it has limits and still see a recurring tendency",
            1_500,
        ),
        pattern_conversation(
            14,
            Speaker::Person,
            "One rushed week still ran differently",
            1_600,
        ),
    ] {
        repository.append_evidence(evidence).unwrap();
    }
    for (claim_id, evidence_id, quote, at) in [
        (1, 1, "I reviewed plans calmly in January", 100),
        (2, 2, "I reviewed plans calmly in February", 200),
        (3, 3, "I reviewed plans calmly in March", 300),
        (4, 4, "I reviewed plans calmly in April", 1_200),
    ] {
        repository
            .append_claim(pattern_claim(claim_id, evidence_id, quote, at))
            .unwrap();
    }
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(1_000));
    let pattern = maintenance
        .propose(
            &MemoryProposal::new("Planning reviews tend to become calmer across months")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claims([1, 2, 3].into_iter().map(ClaimId::from_raw))
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(100)))
                .with_confidence(MemoryConfidence::Medium)
                .with_salience_reason("Worth retaining as a provisional cross-month pattern")
                .with_basis(MemoryBasis::PatternCandidate)
                .with_pattern_counterexample_review(EvidenceCitation::new(
                    EvidenceId::from_raw(10),
                    "I checked the initial sequence for exceptions",
                )),
        )
        .unwrap();
    assert_eq!(pattern.id(), MemoryId::new(1).unwrap());
    assert_eq!(pattern.version(), 1);
    let (repository, _) = maintenance.into_parts();
    repository.close().unwrap();
    VaultRepository::open(directory, VaultKey::new([0x72; 32])).unwrap()
}

fn pattern_conversation(
    id: u64,
    speaker: Speaker,
    text: &str,
    recorded_at_millis: i64,
) -> ConversationEvidence {
    ConversationEvidence::restore(
        EvidenceId::from_raw(id),
        SessionId::new("pattern-maturity-seed"),
        speaker,
        text.to_owned(),
        Timestamp::from_millis(recorded_at_millis),
    )
}

fn pattern_claim(id: u64, evidence_id: u64, quote: &str, recorded_at_millis: i64) -> Claim {
    Claim::restore(
        ClaimId::from_raw(id),
        ClaimOwner::Counterpart,
        format!("planning review event {id}"),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(evidence_id),
            quote,
        )],
        Some(Uncertainty::Medium),
        ApplicableTime::At(Timestamp::from_millis(recorded_at_millis)),
        Timestamp::from_millis(recorded_at_millis),
    )
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
