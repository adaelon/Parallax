use eam_core::{
    ConversationEvidence, EvidenceCitation, EvidenceId, ForgetRepository, ForgetTarget,
    MemoryRepository, ReflectionDecision, ReflectionImportance, ReflectionInvitation,
    ReflectionInvitationBasis, ReflectionInvitationRepository, ReflectionInvitationState,
    SessionId, Speaker, Timestamp, decide_reflection_invitation, offer_reflection_invitation,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x26; 32];
const TOPIC: &str = "工作挤压生活";
const OBSERVATION: &str = "你刚才明确说工作再次挤压了真实生活。";
const WHY_NOW: &str = "这是一项有直接证据的重要变化。";
const EVIDENCE_TEXT: &str = "工作再次挤压了真实生活。";

fn key() -> VaultKey {
    VaultKey::new(TEST_VAULT_KEY)
}

fn append_person_evidence(repository: &mut VaultRepository, verbatim: &str) -> EvidenceId {
    let id = repository.next_evidence_id();
    repository
        .append_evidence(ConversationEvidence::restore(
            id,
            SessionId::new("reflection-persistence"),
            Speaker::Person,
            verbatim.to_owned(),
            Timestamp::from_millis(100),
        ))
        .unwrap();
    id
}

fn pending_invitation(
    repository: &mut VaultRepository,
    evidence_id: EvidenceId,
    topic: &str,
    created_at: i64,
) -> ReflectionInvitation {
    ReflectionInvitation::restore(
        repository.next_reflection_invitation_id(),
        topic,
        OBSERVATION,
        vec![EvidenceCitation::new(evidence_id, EVIDENCE_TEXT)],
        WHY_NOW,
        ReflectionImportance::Important,
        ReflectionInvitationBasis::ImportantSingleChange,
        ReflectionInvitationState::Pending,
        Timestamp::from_millis(created_at),
        Timestamp::from_millis(created_at),
        None,
        None,
        0,
        false,
    )
}

#[test]
fn deferred_invitation_reopens_with_exact_schedule_state() {
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 23);
    let evidence_id = append_person_evidence(&mut repository, EVIDENCE_TEXT);
    let pending = pending_invitation(&mut repository, evidence_id, TOPIC, 200);
    repository
        .commit_reflection_invitation(pending.clone())
        .unwrap();
    let offered = offer_reflection_invitation(&pending, Timestamp::from_millis(300)).unwrap();
    repository
        .transition_reflection_invitation(ReflectionInvitationState::Pending, offered.clone())
        .unwrap();
    let deferred = decide_reflection_invitation(
        &offered,
        ReflectionDecision::Defer,
        Timestamp::from_millis(400),
    )
    .unwrap();
    repository
        .transition_reflection_invitation(ReflectionInvitationState::Offered, deferred.clone())
        .unwrap();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let restored = repository
        .reflection_invitation(deferred.id())
        .unwrap()
        .expect("deferred invitation must survive SQLCipher reopen");
    assert_eq!(restored, deferred);
    assert_eq!(restored.defer_count(), 1);
    assert_eq!(
        restored.next_eligible_at(),
        Some(Timestamp::from_millis(604_800_400))
    );
    assert_eq!(restored.evidence_refs()[0].evidence_id(), evidence_id);
}

#[test]
fn one_open_invitation_per_topic_is_atomic_and_resolved_topic_can_be_reused() {
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first_evidence = append_person_evidence(&mut repository, EVIDENCE_TEXT);
    let second_evidence = append_person_evidence(&mut repository, EVIDENCE_TEXT);
    let first = pending_invitation(&mut repository, first_evidence, TOPIC, 200);
    repository
        .commit_reflection_invitation(first.clone())
        .unwrap();

    let duplicate = pending_invitation(&mut repository, second_evidence, TOPIC, 300);
    assert!(repository.commit_reflection_invitation(duplicate).is_err());
    assert_eq!(
        repository.all_reflection_invitations().unwrap().as_slice(),
        std::slice::from_ref(&first)
    );

    let offered = offer_reflection_invitation(&first, Timestamp::from_millis(400)).unwrap();
    repository
        .transition_reflection_invitation(ReflectionInvitationState::Pending, offered.clone())
        .unwrap();
    let resolved = decide_reflection_invitation(
        &offered,
        ReflectionDecision::Resolve,
        Timestamp::from_millis(500),
    )
    .unwrap();
    repository
        .transition_reflection_invitation(ReflectionInvitationState::Offered, resolved)
        .unwrap();

    let replacement = pending_invitation(&mut repository, second_evidence, TOPIC, 600);
    repository
        .commit_reflection_invitation(replacement.clone())
        .unwrap();
    assert_eq!(repository.all_reflection_invitations().unwrap().len(), 2);
    assert_eq!(
        repository
            .reflection_invitation(replacement.id())
            .unwrap()
            .unwrap(),
        replacement
    );
}

#[test]
fn failed_evidence_link_rolls_back_invitation_and_every_citation() {
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let evidence_id = append_person_evidence(&mut repository, EVIDENCE_TEXT);
    let invitation = pending_invitation(&mut repository, evidence_id, TOPIC, 200);
    let invitation = ReflectionInvitation::restore(
        invitation.id(),
        invitation.topic_key(),
        invitation.observation(),
        vec![
            EvidenceCitation::new(evidence_id, EVIDENCE_TEXT),
            EvidenceCitation::new(EvidenceId::from_raw(999_999), "missing"),
        ],
        invitation.why_now(),
        invitation.importance(),
        invitation.basis(),
        invitation.state(),
        invitation.created_at(),
        invitation.updated_at(),
        invitation.next_eligible_at(),
        invitation.last_offered_at(),
        invitation.defer_count(),
        invitation.mute_prompted(),
    );

    assert!(repository.commit_reflection_invitation(invitation).is_err());
    assert!(repository.all_reflection_invitations().unwrap().is_empty());
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert!(repository.all_reflection_invitations().unwrap().is_empty());
}

#[test]
fn forgetting_supporting_evidence_removes_invitation_across_reopen() {
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let evidence_id = append_person_evidence(&mut repository, EVIDENCE_TEXT);
    let invitation = pending_invitation(&mut repository, evidence_id, TOPIC, 200);
    repository
        .commit_reflection_invitation(invitation.clone())
        .unwrap();

    repository
        .commit_forget(
            ForgetTarget::ConversationEvidence(evidence_id),
            Timestamp::from_millis(300),
        )
        .unwrap()
        .expect("supporting evidence exists");
    assert!(
        repository
            .reflection_invitation(invitation.id())
            .unwrap()
            .is_none()
    );
    assert!(repository.evidence(evidence_id).unwrap().is_none());
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert!(repository.all_reflection_invitations().unwrap().is_empty());
    assert!(repository.evidence(evidence_id).unwrap().is_none());
}
