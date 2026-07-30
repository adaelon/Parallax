use std::{collections::BTreeMap, convert::Infallible};

use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    RetrievedContextItem, SessionId, Speaker, Timestamp,
};
use eam_ingestion::{
    EvidenceBlock, EvidenceBlockId, EvidenceBlockMetadata, EvidenceBlockView, ExtractionRevisionId,
    SourceAnchor,
};
use eam_markdown::MarkdownBlockKind;
use eam_retrieval::{
    AuthoritativeCandidate, AuthoritativeEvidence, CandidateRef, IndexBuildReceipt,
    IndexDisposition, RecallChannels, RecallHit, RetrievalQuery, RetrievalRepository,
    SourceCurrentness, SourceScope, TokenBudget, freeze_working_context, retrieve,
};

#[derive(Clone)]
struct FixtureRepository {
    hits: Vec<RecallHit>,
    memories: Vec<RecallHit>,
    understanding: Vec<RecallHit>,
    neighbors: BTreeMap<CandidateRef, Vec<RecallHit>>,
    authority: BTreeMap<CandidateRef, AuthoritativeCandidate>,
}

impl RetrievalRepository for FixtureRepository {
    type Error = Infallible;

    fn ensure_retrieval_index(&mut self) -> Result<IndexBuildReceipt, Self::Error> {
        Ok(IndexBuildReceipt::new(IndexDisposition::Current, 3, 1, 1))
    }

    fn recall_candidates(&self, _query: &RetrievalQuery) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(self.hits.clone())
    }

    fn recall_long_term_memory_candidates(
        &self,
        _query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(self.memories.clone())
    }

    fn recall_understanding_candidates(
        &self,
        _query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(self.understanding.clone())
    }

    fn recall_neighbors(
        &self,
        reference: CandidateRef,
        _scope: SourceScope,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(self.neighbors.get(&reference).cloned().unwrap_or_default())
    }

    fn resolve_authoritative(
        &self,
        reference: CandidateRef,
        _scope: SourceScope,
    ) -> Result<Option<AuthoritativeCandidate>, Self::Error> {
        Ok(self.authority.get(&reference).cloned())
    }
}

#[test]
fn vector_memory_and_neighbors_freeze_to_one_replayable_budgeted_snapshot() {
    let repository = fixture_repository();
    let query = RetrievalQuery::lexical("coordinating Aurora");
    let result = retrieve(&mut repository.clone(), &query).unwrap();
    assert!(result.candidates().iter().any(|candidate| {
        candidate.channels().contains_vector() && candidate.vector_score_bps() == 8_100
    }));
    assert!(
        result
            .candidates()
            .iter()
            .any(|candidate| candidate.channels().contains_long_term_memory())
    );
    assert!(
        result
            .candidates()
            .iter()
            .any(|candidate| candidate.channels().contains_understanding())
    );

    let first = freeze_working_context(
        &mut repository.clone(),
        &query,
        TokenBudget::new(512).unwrap(),
        selected_conversation(),
        Timestamp::from_millis(500),
    )
    .unwrap();
    let second = freeze_working_context(
        &mut repository.clone(),
        &query,
        TokenBudget::new(512).unwrap(),
        selected_conversation(),
        Timestamp::from_millis(500),
    )
    .unwrap();

    assert_eq!(first, second);
    let snapshot = first.retrieval_snapshot().unwrap();
    assert!(snapshot.used_tokens() <= snapshot.token_budget());
    assert_eq!(
        snapshot.replay_digest(),
        second.retrieval_snapshot().unwrap().replay_digest()
    );
    assert_eq!(first.retrieved().len(), 3);
    let RetrievedContextItem::EvidenceWindow(primary) = &first.retrieved()[0] else {
        panic!("the vector seed must freeze as the first evidence window");
    };
    assert_eq!(
        primary
            .blocks()
            .iter()
            .map(eam_core::FrozenEvidenceBlock::block_id)
            .collect::<Vec<_>>(),
        [11, 12]
    );
}

#[test]
fn one_oversized_authoritative_block_is_skipped_without_truncation() {
    let reference = CandidateRef::Evidence {
        evidence_id: 9,
        block_id: 91,
    };
    let authority = authoritative_evidence(9, 91, 0, &"界".repeat(256), "large.md", 1);
    let mut repository = FixtureRepository {
        hits: vec![RecallHit::vector(reference, 9_000)],
        memories: Vec::new(),
        understanding: Vec::new(),
        neighbors: BTreeMap::new(),
        authority: BTreeMap::from([(reference, authority)]),
    };
    let context = freeze_working_context(
        &mut repository,
        &RetrievalQuery::lexical("界"),
        TokenBudget::new(128).unwrap(),
        Vec::new(),
        Timestamp::from_millis(2),
    )
    .unwrap();

    assert!(context.retrieved().is_empty());
    assert_eq!(context.retrieval_snapshot().unwrap().used_tokens(), 0);
}

fn fixture_repository() -> FixtureRepository {
    let primary = CandidateRef::Evidence {
        evidence_id: 1,
        block_id: 11,
    };
    let adjacent = CandidateRef::Evidence {
        evidence_id: 1,
        block_id: 12,
    };
    let relation = CandidateRef::Evidence {
        evidence_id: 2,
        block_id: 21,
    };
    let memory_source = CandidateRef::Ledger { claim_id: 31 };
    let claim = Claim::restore(
        ClaimId::from_raw(31),
        ClaimOwner::Counterpart,
        "Aurora launch reviews matter".to_owned(),
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "Aurora launch reviews",
        )],
        None,
        ApplicableTime::Since(Timestamp::from_millis(100)),
        Timestamp::from_millis(200),
    );
    FixtureRepository {
        hits: vec![RecallHit::vector(primary, 8_100)],
        memories: vec![RecallHit::new(
            memory_source,
            RecallChannels::long_term_memory(),
            0,
        )],
        understanding: vec![RecallHit::new(relation, RecallChannels::understanding(), 0)],
        neighbors: BTreeMap::from([(
            primary,
            vec![
                RecallHit::new(adjacent, RecallChannels::default(), 0),
                RecallHit::new(relation, RecallChannels::relation(), 0),
            ],
        )]),
        authority: BTreeMap::from([
            (
                primary,
                authoritative_evidence(1, 11, 0, "Coordinate Project Aurora.", "Aurora.md", 100),
            ),
            (
                adjacent,
                authoritative_evidence(
                    1,
                    12,
                    1,
                    "Prepare the weekly launch review.",
                    "Aurora.md",
                    100,
                ),
            ),
            (
                relation,
                authoritative_evidence(2, 21, 0, "Mina owns the launch checklist.", "Mina.md", 101),
            ),
            (memory_source, AuthoritativeCandidate::Ledger(claim)),
        ]),
    }
}

fn authoritative_evidence(
    evidence_id: u64,
    block_id: u64,
    ordinal: usize,
    text: &str,
    locator: &str,
    recorded_at: i64,
) -> AuthoritativeCandidate {
    let block = EvidenceBlock::new(
        EvidenceBlockId::new(block_id).unwrap(),
        evidence_id,
        ExtractionRevisionId::new(evidence_id).unwrap(),
        None,
        ordinal,
        MarkdownBlockKind::Paragraph,
        SourceAnchor::new(text, 0, text.len(), None).unwrap(),
        EvidenceBlockMetadata::new(None, None, None, None),
    )
    .unwrap();
    AuthoritativeCandidate::Evidence(AuthoritativeEvidence::new(
        EvidenceBlockView::new(block, text).unwrap(),
        evidence_id,
        locator.to_owned(),
        SourceCurrentness::Present,
        recorded_at,
    ))
}

fn selected_conversation() -> Vec<ConversationEvidence> {
    vec![ConversationEvidence::restore(
        EvidenceId::from_raw(77),
        SessionId::new("selected"),
        Speaker::Person,
        "Please recall Aurora".to_owned(),
        Timestamp::from_millis(400),
    )]
}
