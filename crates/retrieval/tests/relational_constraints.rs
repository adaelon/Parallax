use eam_core::{
    AgreementWithdrawal, AgreementWithdrawalActor, ApplicableTime, Claim, ClaimId, ClaimOwner,
    SharedAgreementCandidate, SharedAgreementCandidateId, SharedAgreementCandidateStatus,
    SharedExperience, SharedExperienceKind, Timestamp,
};
use eam_retrieval::{RetrievalQuery, project_active_relational_constraints};

fn confirmed_candidate(
    candidate_id: u64,
    claim_id: u64,
    statement: &str,
    scope: &str,
    from: i64,
    until: Option<i64>,
    supersedes: Vec<ClaimId>,
) -> SharedAgreementCandidate {
    SharedAgreementCandidate::restore(
        SharedAgreementCandidateId::from_raw(candidate_id),
        1,
        None,
        statement.to_owned(),
        Some(scope.to_owned()),
        Some(Timestamp::from_millis(from)),
        until.map(Timestamp::from_millis),
        None,
        supersedes,
        Vec::new(),
        Timestamp::from_millis(from),
        Timestamp::from_millis(from),
        SharedAgreementCandidateStatus::Confirmed,
        Some(Timestamp::from_millis(from)),
        Some(Timestamp::from_millis(from)),
        Some(ClaimId::from_raw(claim_id)),
    )
}

fn agreement(claim_id: u64, from: i64, until: Option<i64>) -> SharedExperience {
    let applicable_time =
        until.map_or(ApplicableTime::Since(Timestamp::from_millis(from)), |end| {
            ApplicableTime::Between {
                start: Timestamp::from_millis(from),
                end: Timestamp::from_millis(end),
            }
        });
    SharedExperience::restore(
        SharedExperienceKind::Agreement,
        Claim::restore(
            ClaimId::from_raw(claim_id),
            ClaimOwner::Shared,
            "直接指出关键逃避".to_owned(),
            Vec::new(),
            None,
            applicable_time,
            Timestamp::from_millis(from),
        ),
        true,
    )
}

fn withdrawal(claim_id: u64, agreement_claim_id: u64, effective_at: i64) -> SharedExperience {
    let withdrawal_claim_id = ClaimId::from_raw(claim_id);
    let withdrawal = AgreementWithdrawal::restore(
        withdrawal_claim_id,
        ClaimId::from_raw(agreement_claim_id),
        AgreementWithdrawalActor::Person,
        Timestamp::from_millis(effective_at),
        None,
        Vec::new(),
    );
    SharedExperience::restore_agreement_withdrawal(
        Claim::restore(
            withdrawal_claim_id,
            ClaimOwner::Shared,
            "本人退出共同约定".to_owned(),
            Vec::new(),
            None,
            ApplicableTime::At(Timestamp::from_millis(effective_at)),
            Timestamp::from_millis(effective_at),
        ),
        false,
        withdrawal,
    )
}

#[test]
fn only_scope_relevant_current_confirmed_agreements_are_projected() {
    let candidates = vec![
        confirmed_candidate(
            1,
            11,
            "直接指出关键逃避",
            "双方共同项目复盘",
            1_000,
            None,
            vec![],
        ),
        confirmed_candidate(
            2,
            12,
            "直接指出关键逃避",
            "双方健康议题讨论",
            1_000,
            None,
            vec![],
        ),
        confirmed_candidate(
            3,
            13,
            "直接指出关键逃避",
            "双方共同项目复盘",
            1_000,
            Some(1_500),
            vec![],
        ),
        SharedAgreementCandidate::restore(
            SharedAgreementCandidateId::from_raw(4),
            1,
            None,
            "尚未签署".to_owned(),
            Some("双方共同项目复盘".to_owned()),
            Some(Timestamp::from_millis(1_000)),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Timestamp::from_millis(1_000),
            Timestamp::from_millis(1_000),
            SharedAgreementCandidateStatus::AwaitingPerson,
            Some(Timestamp::from_millis(1_000)),
            None,
            None,
        ),
    ];
    let experiences = vec![
        agreement(11, 1_000, None),
        agreement(12, 1_000, None),
        agreement(13, 1_000, Some(1_500)),
    ];

    let relevant = project_active_relational_constraints(
        &RetrievalQuery::lexical("请准备这次共同项目复盘"),
        &candidates,
        &experiences,
        Timestamp::from_millis(2_000),
    );
    assert_eq!(relevant.len(), 1);
    assert_eq!(relevant[0].agreement_claim_id(), ClaimId::from_raw(11));
    assert_eq!(relevant[0].scope(), "双方共同项目复盘");

    let unrelated = project_active_relational_constraints(
        &RetrievalQuery::lexical("请整理旅行照片"),
        &candidates,
        &experiences,
        Timestamp::from_millis(2_000),
    );
    assert!(unrelated.is_empty());
    let relational_but_unrelated = project_active_relational_constraints(
        &RetrievalQuery::lexical("请安排双方共同旅行"),
        &candidates,
        &experiences,
        Timestamp::from_millis(2_000),
    );
    assert!(relational_but_unrelated.is_empty());
    let overlapping_but_unrelated = project_active_relational_constraints(
        &RetrievalQuery::lexical("请做一次个人复盘"),
        &candidates,
        &experiences,
        Timestamp::from_millis(2_000),
    );
    assert!(overlapping_but_unrelated.is_empty());
}

#[test]
fn an_orphaned_or_not_yet_effective_agreement_never_becomes_a_constraint() {
    let candidates = vec![confirmed_candidate(
        1,
        11,
        "直接指出关键逃避",
        "双方共同项目复盘",
        5_000,
        None,
        vec![],
    )];
    let query = RetrievalQuery::lexical("共同项目复盘");
    assert!(
        project_active_relational_constraints(
            &query,
            &candidates,
            &[],
            Timestamp::from_millis(6_000),
        )
        .is_empty()
    );
    assert!(
        project_active_relational_constraints(
            &query,
            &candidates,
            &[agreement(11, 5_000, None)],
            Timestamp::from_millis(4_999),
        )
        .is_empty()
    );
}

#[test]
fn whole_supersession_changes_only_future_projection_and_keeps_compatible_agreements() {
    let candidates = vec![
        confirmed_candidate(
            1,
            11,
            "复盘时直接指出关键逃避",
            "双方共同项目复盘",
            1_000,
            None,
            vec![],
        ),
        confirmed_candidate(
            2,
            12,
            "复盘时不要直接指出关键逃避",
            "双方共同项目复盘",
            5_000,
            Some(6_000),
            vec![ClaimId::from_raw(11)],
        ),
        confirmed_candidate(
            3,
            13,
            "复盘后记录结论",
            "双方共同项目复盘",
            1_000,
            None,
            vec![],
        ),
    ];
    let experiences = vec![
        agreement(11, 1_000, None),
        agreement(12, 5_000, Some(6_000)),
        agreement(13, 1_000, None),
    ];
    let query = RetrievalQuery::lexical("共同项目复盘");

    let before = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(4_999),
    );
    assert_eq!(
        before
            .iter()
            .map(eam_core::ActiveRelationalConstraint::agreement_claim_id)
            .collect::<Vec<_>>(),
        vec![ClaimId::from_raw(11), ClaimId::from_raw(13)]
    );

    let after = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(5_000),
    );
    assert_eq!(
        after
            .iter()
            .map(eam_core::ActiveRelationalConstraint::agreement_claim_id)
            .collect::<Vec<_>>(),
        vec![ClaimId::from_raw(12), ClaimId::from_raw(13)]
    );

    let after_replacement_ends = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(6_001),
    );
    assert_eq!(
        after_replacement_ends
            .iter()
            .map(eam_core::ActiveRelationalConstraint::agreement_claim_id)
            .collect::<Vec<_>>(),
        vec![ClaimId::from_raw(13)],
        "a superseded agreement must not revive when its replacement ends"
    );
    assert_eq!(experiences.len(), 3, "supersession must not delete history");
}

#[test]
fn withdrawal_stops_only_future_projection_and_keeps_agreement_history() {
    let candidates = vec![confirmed_candidate(
        1,
        11,
        "复盘时直接指出关键逃避",
        "双方共同项目复盘",
        1_000,
        None,
        vec![],
    )];
    let experiences = vec![agreement(11, 1_000, None), withdrawal(21, 11, 5_000)];
    let query = RetrievalQuery::lexical("共同项目复盘");

    let before = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(4_999),
    );
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].agreement_claim_id(), ClaimId::from_raw(11));

    let effective = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(5_000),
    );
    assert!(effective.is_empty());
    assert_eq!(experiences.len(), 2, "withdrawal must not delete history");
}
