fn conversation(id: u64, speaker: Speaker, text: &str) -> ConversationEvidence {
    ConversationEvidence::restore(
        EvidenceId::from_raw(id),
        SessionId::new("memory-proposal-test"),
        speaker,
        text.to_owned(),
        Timestamp::from_millis(i64::try_from(id).unwrap() * 10),
    )
}
use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ClaimStatus, ConversationEvidence,
    EvidenceCitation, EvidenceId, IncrementingClock, SessionId, Speaker, Timestamp, Uncertainty,
};
use eam_memory::{
    InMemoryLongTermMemoryRepository, LongTermMemoryRepository, MemoryBasis, MemoryConfidence,
    MemoryError, MemoryKind, MemoryMaintenance, MemoryProposal, MemoryProposalRejectionReason,
    MemoryStatus, MemorySubject,
};

#[test]
fn explicit_direct_and_inferential_proposals_choose_bounded_initial_statuses() {
    let repository = proposal_fixture_repository();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(1_000));

    let active = maintenance
        .propose(
            &complete_proposal(
                "I live in Shenzhen",
                MemorySubject::Person,
                ClaimId::from_raw(1),
            )
            .with_kind(MemoryKind::Fact)
            .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
            .with_confidence(MemoryConfidence::High)
            .with_basis(MemoryBasis::DirectEvidence),
        )
        .unwrap();
    assert_eq!(active.version(), 1);
    assert_eq!(active.status(), MemoryStatus::Active);

    let provisional = maintenance
        .propose(
            &complete_proposal(
                "I should retain the possibility that planning is becoming steadier",
                MemorySubject::Counterpart,
                ClaimId::from_raw(2),
            )
            .with_kind(MemoryKind::Hypothesis)
            .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(20)))
            .with_confidence(MemoryConfidence::Medium)
            .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    assert_eq!(provisional.status(), MemoryStatus::Provisional);
    assert_ne!(active.id(), provisional.id());

    let pattern = maintenance
        .propose(
            &MemoryProposal::new("A recurring planning-review pattern may be emerging")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claims([
                    ClaimId::from_raw(3),
                    ClaimId::from_raw(4),
                    ClaimId::from_raw(5),
                ])
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(30)))
                .with_confidence(MemoryConfidence::High)
                .with_salience_reason("Worth retaining for later pattern qualification")
                .with_basis(MemoryBasis::PatternCandidate)
                .with_pattern_counterexample_review(EvidenceCitation::new(
                    EvidenceId::from_raw(6),
                    "I checked these three planning reviews for counterexamples",
                )),
        )
        .unwrap();
    assert_eq!(pattern.status(), MemoryStatus::ProvisionalPattern);
}

fn proposal_fixture_repository() -> InMemoryLongTermMemoryRepository {
    let evidence = [
        conversation(3, Speaker::Person, "Planning review one"),
        conversation(4, Speaker::Person, "Planning review two"),
        conversation(5, Speaker::Person, "Planning review three"),
        conversation(
            6,
            Speaker::Counterpart,
            "I checked these three planning reviews for counterexamples",
        ),
    ];
    let claims = [
        claim(
            1,
            ClaimOwner::Person,
            "I live in Shenzhen",
            None,
            ApplicableTime::Since(Timestamp::from_millis(10)),
        ),
        claim(
            2,
            ClaimOwner::Counterpart,
            "Their planning rhythm appears steadier",
            Some(Uncertainty::Medium),
            ApplicableTime::Since(Timestamp::from_millis(20)),
        ),
        claim(
            3,
            ClaimOwner::Counterpart,
            "Planning review one",
            Some(Uncertainty::Low),
            ApplicableTime::At(Timestamp::from_millis(30)),
        ),
        claim(
            4,
            ClaimOwner::Counterpart,
            "Planning review two",
            Some(Uncertainty::Low),
            ApplicableTime::At(Timestamp::from_millis(40)),
        ),
        claim(
            5,
            ClaimOwner::Counterpart,
            "Planning review three",
            Some(Uncertainty::Low),
            ApplicableTime::At(Timestamp::from_millis(50)),
        ),
    ];
    InMemoryLongTermMemoryRepository::with_evidence_and_claims(evidence, claims).unwrap()
}

#[test]
fn missing_fields_and_cross_ledger_subjects_are_rejected_without_writes() {
    let repository = InMemoryLongTermMemoryRepository::with_claims([
        claim(
            1,
            ClaimOwner::Person,
            "I prefer quiet mornings",
            None,
            ApplicableTime::Unknown,
        ),
        claim(
            2,
            ClaimOwner::Counterpart,
            "This may be a fragile inference",
            Some(Uncertainty::Medium),
            ApplicableTime::Unknown,
        ),
    ])
    .unwrap();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));

    let missing = maintenance.propose(&MemoryProposal::new("Incomplete"));
    assert!(matches!(
        missing,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::MissingSubject
        ))
    ));

    let crossed = maintenance.propose(
        &complete_proposal(
            "I prefer quiet mornings",
            MemorySubject::Counterpart,
            ClaimId::from_raw(1),
        )
        .with_kind(MemoryKind::Preference)
        .with_applicable_time(ApplicableTime::Unknown)
        .with_confidence(MemoryConfidence::High)
        .with_basis(MemoryBasis::DirectEvidence),
    );
    assert!(matches!(
        crossed,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::CrossLedgerSubject { .. }
        ))
    ));
    let missing_time = maintenance.propose(
        &complete_proposal(
            "I prefer quiet mornings",
            MemorySubject::Person,
            ClaimId::from_raw(1),
        )
        .with_kind(MemoryKind::Preference)
        .with_confidence(MemoryConfidence::High)
        .with_basis(MemoryBasis::DirectEvidence),
    );
    assert!(matches!(
        missing_time,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::MissingApplicableTime
        ))
    ));
    let overconfident = maintenance.propose(
        &complete_proposal(
            "A stronger conclusion than the source supports",
            MemorySubject::Counterpart,
            ClaimId::from_raw(2),
        )
        .with_kind(MemoryKind::Hypothesis)
        .with_applicable_time(ApplicableTime::Unknown)
        .with_confidence(MemoryConfidence::High)
        .with_basis(MemoryBasis::InterpretiveInference),
    );
    assert!(matches!(
        overconfident,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::ConfidenceExceedsSource(id)
        )) if id == ClaimId::from_raw(2)
    ));
    assert!(
        maintenance
            .repository()
            .all_memory_versions()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn superseded_claim_cannot_seed_a_new_long_term_memory() {
    let old = Claim::restore_versioned(
        ClaimId::from_raw(1),
        ClaimOwner::Person,
        "I live in Shenzhen".to_owned(),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "I live in Shenzhen",
        )],
        None,
        ApplicableTime::Since(Timestamp::from_millis(10)),
        Timestamp::from_millis(10),
        ClaimStatus::Superseded,
        None,
        Some(ClaimId::from_raw(2)),
    );
    let repository = InMemoryLongTermMemoryRepository::with_claims([old]).unwrap();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(1_000));

    assert_eq!(
        maintenance.propose(
            &complete_proposal(
                "I live in Shenzhen",
                MemorySubject::Person,
                ClaimId::from_raw(1),
            )
            .with_kind(MemoryKind::Fact)
            .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
            .with_confidence(MemoryConfidence::High)
            .with_basis(MemoryBasis::DirectEvidence),
        ),
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::SourceNotCurrent(ClaimId::from_raw(1))
        ))
    );
    assert!(
        maintenance
            .repository()
            .all_memory_versions()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ledger_entries_do_not_create_memories_without_a_proposal() {
    let repository = InMemoryLongTermMemoryRepository::with_claims([claim(
        1,
        ClaimOwner::Shared,
        "We agreed to revisit this next month",
        None,
        ApplicableTime::At(Timestamp::from_millis(30)),
    )])
    .unwrap();
    assert!(repository.all_memory_versions().unwrap().is_empty());
}

#[test]
fn explicit_revision_appends_a_version_and_supersedes_the_predecessor() {
    let repository = InMemoryLongTermMemoryRepository::with_claims([
        claim(
            1,
            ClaimOwner::Person,
            "I live in Shenzhen",
            None,
            ApplicableTime::Since(Timestamp::from_millis(10)),
        ),
        claim(
            2,
            ClaimOwner::Person,
            "I live in Guangzhou",
            None,
            ApplicableTime::Since(Timestamp::from_millis(40)),
        ),
    ])
    .unwrap();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(3_000));
    let first = maintenance
        .propose(
            &complete_proposal(
                "I live in Shenzhen",
                MemorySubject::Person,
                ClaimId::from_raw(1),
            )
            .with_kind(MemoryKind::Fact)
            .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
            .with_confidence(MemoryConfidence::High)
            .with_basis(MemoryBasis::DirectEvidence),
        )
        .unwrap();
    let second = maintenance
        .propose(
            &complete_proposal(
                "I live in Guangzhou",
                MemorySubject::Person,
                ClaimId::from_raw(2),
            )
            .with_kind(MemoryKind::Fact)
            .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(40)))
            .with_confidence(MemoryConfidence::High)
            .with_basis(MemoryBasis::DirectEvidence)
            .revising(first.id(), first.version()),
        )
        .unwrap();

    assert_eq!(second.id(), first.id());
    assert_eq!(second.version(), 2);
    assert_eq!(second.predecessor_version(), Some(1));
    let versions = maintenance
        .repository()
        .memory_versions(first.id())
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].status(), MemoryStatus::Superseded);
    assert_eq!(versions[1], second);
}

fn complete_proposal(statement: &str, subject: MemorySubject, claim_id: ClaimId) -> MemoryProposal {
    MemoryProposal::new(statement)
        .with_subject(subject)
        .with_source_claim(claim_id)
        .with_salience_reason("Needed across future conversations")
}

fn claim(
    id: u64,
    owner: ClaimOwner,
    statement: &str,
    uncertainty: Option<Uncertainty>,
    applicable_time: ApplicableTime,
) -> Claim {
    Claim::restore(
        ClaimId::from_raw(id),
        owner,
        statement.to_owned(),
        vec![EvidenceCitation::new(EvidenceId::from_raw(id), statement)],
        uncertainty,
        applicable_time,
        Timestamp::from_millis(i64::try_from(id).unwrap()),
    )
}
