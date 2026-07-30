use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    IncrementingClock, SessionId, Speaker, Timestamp, Uncertainty,
};
use eam_memory::{
    InMemoryLongTermMemoryRepository, LongTermMemoryRepository, MemoryBasis, MemoryConfidence,
    MemoryDisputeOutcome, MemoryDisputeRejectionReason, MemoryDisputeRequest, MemoryDisputeReview,
    MemoryError, MemoryKind, MemoryMaintenance, MemoryProposal, MemoryProposalRejectionReason,
    MemoryStatus, MemorySubject,
};

#[test]
fn unpersuaded_review_keeps_the_memory_disputed_with_both_positions_and_evidence() {
    let mut maintenance = fixture();
    let memory = maintenance.propose(&original_proposal()).unwrap();

    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                memory.id(),
                memory.version(),
                "I think one unusually busy week is being overgeneralized",
            )
            .with_counter_evidence(citation(2, "That week was exceptional")),
        )
        .unwrap();

    assert_eq!(dispute.outcome(), MemoryDisputeOutcome::Open);
    assert_eq!(
        maintenance
            .repository()
            .current_memory(memory.id())
            .unwrap()
            .unwrap()
            .status(),
        MemoryStatus::Disputed
    );

    let resolution = maintenance
        .review_dispute(
            &MemoryDisputeReview::maintain(
                dispute.id(),
                "The broader sequence still supports my interpretation",
            )
            .with_evidence(citation(3, "The pattern continued after that week")),
        )
        .unwrap();

    assert_eq!(
        resolution.dispute().outcome(),
        MemoryDisputeOutcome::Maintained
    );
    assert_eq!(resolution.memory().status(), MemoryStatus::Disputed);
    assert_eq!(resolution.dispute().counter_evidence().len(), 1);
    assert_eq!(resolution.dispute().review().unwrap().evidence().len(), 1);
}

#[test]
fn retracted_claim_cannot_be_reproposed_without_a_new_source() {
    let mut maintenance = fixture();
    let memory = maintenance.propose(&original_proposal()).unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                memory.id(),
                memory.version(),
                "I found a concrete exception",
            )
            .with_counter_evidence(citation(2, "That week was exceptional")),
        )
        .unwrap();
    let resolution = maintenance
        .review_dispute(
            &MemoryDisputeReview::retract(
                dispute.id(),
                "The objection invalidates the retained generalization",
            )
            .with_evidence(citation(3, "The pattern continued after that week")),
        )
        .unwrap();
    assert_eq!(resolution.memory().status(), MemoryStatus::Retracted);

    let repeated = maintenance.propose(&original_proposal());
    assert!(matches!(
        repeated,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::RetractedClaimRequiresNewEvidence(id)
        )) if id == memory.id()
    ));

    let with_new_source = maintenance
        .propose(
            &original_proposal()
                .with_source_claim(ClaimId::from_raw(4))
                .with_confidence(MemoryConfidence::Medium),
        )
        .unwrap();
    assert_eq!(with_new_source.status(), MemoryStatus::Provisional);
}

#[test]
fn persuaded_revision_supersedes_the_disputed_version_atomically() {
    let mut maintenance = fixture();
    let memory = maintenance.propose(&original_proposal()).unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                memory.id(),
                memory.version(),
                "The conclusion is too broad",
            )
            .with_counter_evidence(citation(2, "That week was exceptional")),
        )
        .unwrap();
    let revised = MemoryProposal::new("Busy-week planning can become less steady")
        .with_subject(MemorySubject::Counterpart)
        .with_kind(MemoryKind::Hypothesis)
        .with_source_claims([ClaimId::from_raw(1), ClaimId::from_raw(4)])
        .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
        .with_confidence(MemoryConfidence::Medium)
        .with_salience_reason("Retain the narrower, evidence-bounded interpretation")
        .with_basis(MemoryBasis::InterpretiveInference)
        .revising(memory.id(), memory.version());

    let resolution = maintenance
        .review_dispute(
            &MemoryDisputeReview::revise(
                dispute.id(),
                "The objection supports a narrower interpretation",
                revised,
            )
            .with_evidence(citation(3, "The pattern continued after that week")),
        )
        .unwrap();

    assert_eq!(
        resolution.dispute().outcome(),
        MemoryDisputeOutcome::Revised
    );
    assert_eq!(resolution.dispute().revised_version(), Some(2));
    assert_eq!(resolution.memory().version(), 2);
    assert_eq!(resolution.memory().status(), MemoryStatus::Provisional);
    let versions = maintenance
        .repository()
        .memory_versions(memory.id())
        .unwrap();
    assert_eq!(versions[0].status(), MemoryStatus::Superseded);
    assert_eq!(versions[1], *resolution.memory());
}

#[test]
fn dispute_without_counter_evidence_is_rejected_without_changing_memory_state() {
    let mut maintenance = fixture();
    let memory = maintenance.propose(&original_proposal()).unwrap();

    let result = maintenance.raise_dispute(&MemoryDisputeRequest::new(
        memory.id(),
        memory.version(),
        "I disagree",
    ));

    assert!(matches!(
        result,
        Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::MissingCounterEvidence
        ))
    ));
    assert_eq!(
        maintenance
            .repository()
            .current_memory(memory.id())
            .unwrap()
            .unwrap()
            .status(),
        MemoryStatus::Provisional
    );
}

fn fixture() -> MemoryMaintenance<InMemoryLongTermMemoryRepository, IncrementingClock> {
    let evidence = [
        conversation(1, Speaker::Counterpart, "Planning has become steadier"),
        conversation(2, Speaker::Person, "That week was exceptional"),
        conversation(
            3,
            Speaker::Counterpart,
            "The pattern continued after that week",
        ),
        conversation(
            4,
            Speaker::Counterpart,
            "Busy-week planning was less steady",
        ),
    ];
    let claims = [
        claim(
            1,
            "Planning has become steadier",
            Some(Uncertainty::Medium),
            1,
        ),
        claim(
            4,
            "Busy-week planning was less steady",
            Some(Uncertainty::Medium),
            4,
        ),
    ];
    let repository =
        InMemoryLongTermMemoryRepository::with_evidence_and_claims(evidence, claims).unwrap();
    MemoryMaintenance::new(repository, IncrementingClock::new(1_000))
}

fn original_proposal() -> MemoryProposal {
    MemoryProposal::new("Planning has become steadier")
        .with_subject(MemorySubject::Counterpart)
        .with_kind(MemoryKind::Hypothesis)
        .with_source_claim(ClaimId::from_raw(1))
        .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
        .with_confidence(MemoryConfidence::Medium)
        .with_salience_reason("Useful in future planning conversations")
        .with_basis(MemoryBasis::InterpretiveInference)
}

fn conversation(id: u64, speaker: Speaker, text: &str) -> ConversationEvidence {
    ConversationEvidence::restore(
        EvidenceId::from_raw(id),
        SessionId::new("memory-dispute-test"),
        speaker,
        text.to_owned(),
        Timestamp::from_millis(i64::try_from(id).unwrap()),
    )
}

fn claim(id: u64, statement: &str, uncertainty: Option<Uncertainty>, evidence_id: u64) -> Claim {
    Claim::restore(
        ClaimId::from_raw(id),
        ClaimOwner::Counterpart,
        statement.to_owned(),
        vec![citation(evidence_id, statement)],
        uncertainty,
        ApplicableTime::Since(Timestamp::from_millis(10)),
        Timestamp::from_millis(i64::try_from(id).unwrap()),
    )
}

fn citation(id: u64, quote: &str) -> EvidenceCitation {
    EvidenceCitation::new(EvidenceId::from_raw(id), quote)
}
