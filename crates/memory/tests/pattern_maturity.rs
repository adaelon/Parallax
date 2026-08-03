use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    IncrementingClock, PatternMaturityProposal, SessionId, Speaker, Timestamp, Uncertainty,
};
use eam_memory::{
    InMemoryLongTermMemoryRepository, LongTermMemoryRepository, MemoryBasis, MemoryConfidence,
    MemoryDisputeRequest, MemoryDisputeReview, MemoryError, MemoryKind, MemoryMaintenance,
    MemoryProposal, MemoryProposalRejectionReason, MemoryStatus, MemorySubject,
    PatternMaturityRejectionReason,
};

#[test]
fn pattern_candidate_requires_three_independent_events_and_a_counterexample_review() {
    let mut two_events = maintenance_with_claim_supports([1, 2, 3], [1, 2, 3]);
    let two_event_result = two_events.propose(&pattern_proposal([1, 2], true));
    assert_eq!(
        two_event_result,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternRequiresThreeIndependentEvents,
        ))
    );

    let mut duplicate_source = maintenance_with_claim_supports([1, 2, 3], [1, 1, 1]);
    let duplicate_result = duplicate_source.propose(&pattern_proposal([1, 2, 3], true));
    assert_eq!(
        duplicate_result,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternRequiresThreeIndependentEvents,
        ))
    );

    let mut no_review = maintenance_with_claim_supports([1, 2, 3], [1, 2, 3]);
    let no_review_result = no_review.propose(&pattern_proposal([1, 2, 3], false));
    assert_eq!(
        no_review_result,
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternMissingCounterexampleReview,
        ))
    );
    assert!(
        no_review
            .repository()
            .all_memory_versions()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn maturity_eligibility_never_upgrades_without_an_explicit_counterpart_proposal() {
    let mut maintenance = complete_fixture();
    let pattern = maintenance
        .propose(&pattern_proposal([1, 2, 3], true))
        .unwrap();

    assert_eq!(pattern.status(), MemoryStatus::ProvisionalPattern);
    assert_eq!(
        maintenance
            .repository()
            .current_memory(pattern.id())
            .unwrap()
            .unwrap()
            .status(),
        MemoryStatus::ProvisionalPattern
    );
    assert!(
        maintenance
            .repository()
            .pattern_maturity_records(pattern.id())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn maturity_rejects_each_missing_qualification_input_without_writing() {
    let cases = [
        (
            PatternMaturityProposal::new(1, 1, "The longer sequence now supports a stable view")
                .with_counterexample_review(citation(
                    11,
                    "I checked the newer sequence for exceptions",
                ))
                .with_discussion_evidence([
                    citation(
                        12,
                        "I think that pattern fits some weeks, but not every week",
                    ),
                    citation(
                        13,
                        "I agree it has limits and still see a recurring tendency",
                    ),
                ]),
            PatternMaturityRejectionReason::MissingNewSupport,
        ),
        (
            PatternMaturityProposal::new(1, 1, "The longer sequence now supports a stable view")
                .with_new_support_claim(ClaimId::from_raw(4))
                .with_discussion_evidence([
                    citation(
                        12,
                        "I think that pattern fits some weeks, but not every week",
                    ),
                    citation(
                        13,
                        "I agree it has limits and still see a recurring tendency",
                    ),
                ]),
            PatternMaturityRejectionReason::MissingCounterexampleReview,
        ),
        (
            PatternMaturityProposal::new(1, 1, "The longer sequence now supports a stable view")
                .with_new_support_claim(ClaimId::from_raw(4))
                .with_counterexample_review(citation(
                    11,
                    "I checked the newer sequence for exceptions",
                ))
                .with_discussion_evidence([citation(
                    13,
                    "I agree it has limits and still see a recurring tendency",
                )]),
            PatternMaturityRejectionReason::DiscussionRequiresPerson,
        ),
    ];

    for (proposal, expected) in cases {
        let mut maintenance = complete_fixture();
        let pattern = maintenance
            .propose(&pattern_proposal([1, 2, 3], true))
            .unwrap();
        assert_eq!(pattern.id().get(), proposal.memory_id());
        assert_eq!(
            maintenance.mature_pattern(&proposal),
            Err(MemoryError::InvalidPatternMaturity(expected))
        );
        assert_eq!(
            maintenance
                .repository()
                .current_memory(pattern.id())
                .unwrap()
                .unwrap()
                .status(),
            MemoryStatus::ProvisionalPattern
        );
    }
}

#[test]
fn explicit_maturity_proposal_appends_a_supported_counterpart_view_version() {
    let mut maintenance = complete_fixture();
    let pattern = maintenance
        .propose(&pattern_proposal([1, 2, 3], true))
        .unwrap();

    let matured = maintenance
        .mature_pattern(
            &PatternMaturityProposal::new(
                pattern.id().get(),
                pattern.version(),
                "The newer independent support survived review and discussion",
            )
            .with_new_support_claim(ClaimId::from_raw(4))
            .with_counter_evidence(citation(14, "One rushed week still ran differently"))
            .with_counterexample_review(citation(11, "I checked the newer sequence for exceptions"))
            .with_discussion_evidence([
                citation(
                    12,
                    "I think that pattern fits some weeks, but not every week",
                ),
                citation(
                    13,
                    "I agree it has limits and still see a recurring tendency",
                ),
            ]),
        )
        .unwrap();

    assert_eq!(matured.id(), pattern.id());
    assert_eq!(matured.version(), 2);
    assert_eq!(matured.predecessor_version(), Some(1));
    assert_eq!(matured.status(), MemoryStatus::SupportedCounterpartView);
    assert_eq!(matured.subject(), MemorySubject::Counterpart);
    assert_eq!(
        matured.source_claim_ids(),
        &[
            ClaimId::from_raw(1),
            ClaimId::from_raw(2),
            ClaimId::from_raw(3),
            ClaimId::from_raw(4),
        ]
    );
    let records = maintenance
        .repository()
        .pattern_maturity_records(pattern.id())
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].from_version(), 1);
    assert_eq!(records[0].to_version(), 2);
    assert_eq!(records[0].discussion_evidence().len(), 2);
}

#[test]
fn a_strong_counterexample_can_weaken_a_supported_view() {
    let mut maintenance = complete_fixture();
    let pattern = maintenance
        .propose(&pattern_proposal([1, 2, 3], true))
        .unwrap();
    let matured = maintenance
        .mature_pattern(
            &PatternMaturityProposal::new(pattern.id().get(), 1, "Stable after review")
                .with_new_support_claim(ClaimId::from_raw(4))
                .with_counterexample_review(citation(
                    11,
                    "I checked the newer sequence for exceptions",
                ))
                .with_discussion_evidence([
                    citation(
                        12,
                        "I think that pattern fits some weeks, but not every week",
                    ),
                    citation(
                        13,
                        "I agree it has limits and still see a recurring tendency",
                    ),
                ]),
        )
        .unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                matured.id(),
                matured.version(),
                "This stronger exception changes how broadly the view should be used",
            )
            .with_counter_evidence(citation(14, "One rushed week still ran differently")),
        )
        .unwrap();
    let resolution = maintenance
        .review_dispute(
            &MemoryDisputeReview::weaken(
                dispute.id(),
                "The counterexample weakens but does not erase the longer observation",
            )
            .with_evidence(citation(
                13,
                "I agree it has limits and still see a recurring tendency",
            )),
        )
        .unwrap();

    assert_eq!(resolution.memory().status(), MemoryStatus::Weakened);
}

fn complete_fixture() -> MemoryMaintenance<InMemoryLongTermMemoryRepository, IncrementingClock> {
    maintenance_with_claim_supports([1, 2, 3], [1, 2, 3])
}

fn maintenance_with_claim_supports(
    claim_ids: [u64; 3],
    evidence_ids: [u64; 3],
) -> MemoryMaintenance<InMemoryLongTermMemoryRepository, IncrementingClock> {
    let evidence = [
        conversation(
            1,
            Speaker::Person,
            "I reviewed plans calmly in January",
            100,
        ),
        conversation(
            2,
            Speaker::Person,
            "I reviewed plans calmly in February",
            200,
        ),
        conversation(3, Speaker::Person, "I reviewed plans calmly in March", 300),
        conversation(
            4,
            Speaker::Person,
            "I reviewed plans calmly in April",
            1_200,
        ),
        conversation(
            10,
            Speaker::Counterpart,
            "I checked the initial sequence for exceptions",
            350,
        ),
        conversation(
            11,
            Speaker::Counterpart,
            "I checked the newer sequence for exceptions",
            1_300,
        ),
        conversation(
            12,
            Speaker::Person,
            "I think that pattern fits some weeks, but not every week",
            1_400,
        ),
        conversation(
            13,
            Speaker::Counterpart,
            "I agree it has limits and still see a recurring tendency",
            1_500,
        ),
        conversation(
            14,
            Speaker::Person,
            "One rushed week still ran differently",
            1_600,
        ),
    ];
    let claims = claim_ids
        .into_iter()
        .zip(evidence_ids)
        .map(|(claim_id, evidence_id)| pattern_claim(claim_id, evidence_id))
        .chain(std::iter::once(pattern_claim(4, 4)))
        .collect::<Vec<_>>();
    let repository =
        InMemoryLongTermMemoryRepository::with_evidence_and_claims(evidence, claims).unwrap();
    MemoryMaintenance::new(repository, IncrementingClock::new(1_000))
}

fn pattern_proposal<const N: usize>(claim_ids: [u64; N], reviewed: bool) -> MemoryProposal {
    let mut proposal = MemoryProposal::new("Planning reviews tend to become calmer across months")
        .with_subject(MemorySubject::Counterpart)
        .with_kind(MemoryKind::Hypothesis)
        .with_source_claims(claim_ids.into_iter().map(ClaimId::from_raw))
        .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(100)))
        .with_confidence(MemoryConfidence::Medium)
        .with_salience_reason("Worth retaining as a provisional cross-month pattern")
        .with_basis(MemoryBasis::PatternCandidate);
    if reviewed {
        proposal = proposal.with_pattern_counterexample_review(citation(
            10,
            "I checked the initial sequence for exceptions",
        ));
    }
    proposal
}

fn pattern_claim(id: u64, evidence_id: u64) -> Claim {
    let text = format!("planning review event {id}");
    Claim::restore(
        ClaimId::from_raw(id),
        ClaimOwner::Counterpart,
        text.clone(),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(evidence_id),
            match evidence_id {
                2 => "I reviewed plans calmly in February",
                3 => "I reviewed plans calmly in March",
                4 => "I reviewed plans calmly in April",
                _ => "I reviewed plans calmly in January",
            },
        )],
        Some(Uncertainty::Medium),
        ApplicableTime::At(Timestamp::from_millis(
            i64::try_from(evidence_id).unwrap() * 100,
        )),
        Timestamp::from_millis(i64::try_from(evidence_id).unwrap() * 100),
    )
}

fn conversation(id: u64, speaker: Speaker, text: &str, at: i64) -> ConversationEvidence {
    ConversationEvidence::restore(
        EvidenceId::from_raw(id),
        SessionId::new("pattern-maturity-test"),
        speaker,
        text.to_owned(),
        Timestamp::from_millis(at),
    )
}

fn citation(id: u64, quote: &str) -> EvidenceCitation {
    EvidenceCitation::new(EvidenceId::from_raw(id), quote)
}
