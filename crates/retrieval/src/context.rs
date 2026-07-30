use std::{collections::BTreeSet, error::Error, fmt};

use eam_core::{
    ApplicableTime, ConversationEvidence, FrozenEvidenceBlock, FrozenLedgerClaim,
    FrozenMemoryDispute, FrozenRetrievalWindow, RetrievalSnapshot, RetrievedContextItem,
    SourceCurrentness as CoreSourceCurrentness, Timestamp, WorkingContext, WorkingContextError,
};
use sha2::{Digest, Sha256};

use crate::{
    AuthoritativeCandidate, AuthoritativeEvidence, CandidateRef, DisputedMemoryRecall,
    EMBEDDING_MODEL_VERSION, RETRIEVAL_INDEX_VERSION, RecallHit, RetrievalFailure, RetrievalQuery,
    RetrievalRepository, SourceCurrentness, retrieve,
};

pub const DEFAULT_TOKEN_BUDGET: usize = 4_096;
pub const MIN_TOKEN_BUDGET: usize = 128;
pub const MAX_TOKEN_BUDGET: usize = 32_768;

const WINDOW_METADATA_TOKENS: usize = 24;
const BLOCK_METADATA_TOKENS: usize = 16;
const CLAIM_METADATA_TOKENS: usize = 24;
const DISPUTE_METADATA_TOKENS: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenBudget(usize);

impl TokenBudget {
    /// Creates one bounded deterministic context budget.
    ///
    /// # Errors
    ///
    /// Returns [`crate::RetrievalError::InvalidTokenBudget`] outside the G07
    /// range of 128 through 32,768 estimated tokens.
    pub const fn new(value: usize) -> Result<Self, crate::RetrievalError> {
        if value < MIN_TOKEN_BUDGET || value > MAX_TOKEN_BUDGET {
            return Err(crate::RetrievalError::InvalidTokenBudget);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self(DEFAULT_TOKEN_BUDGET)
    }
}

/// Retrieves, authority-resolves, expands, budgets, and freezes one replayable
/// working context without exposing repository or vector-index state.
///
/// # Errors
///
/// Returns query, repository, or Core snapshot validation failures without
/// returning a partial context.
pub fn freeze_working_context<R: RetrievalRepository>(
    repository: &mut R,
    query: &RetrievalQuery,
    budget: TokenBudget,
    selected_conversation: Vec<ConversationEvidence>,
    frozen_at: Timestamp,
) -> Result<WorkingContext, FreezeFailure<R::Error>> {
    let result = retrieve(repository, query).map_err(FreezeFailure::Retrieval)?;
    let mut items = Vec::new();
    let mut used_tokens = 0_usize;
    let mut selected_refs = BTreeSet::new();

    for dispute in result.disputed_memories() {
        let cost = estimate_dispute_tokens(dispute);
        if used_tokens.saturating_add(cost) > budget.get() {
            continue;
        }
        used_tokens += cost;
        items.push(RetrievedContextItem::MemoryDispute(
            FrozenMemoryDispute::new(
                dispute.dispute_id(),
                dispute.memory_id(),
                dispute.memory_version(),
                dispute.counterpart_view().to_owned(),
                dispute.counterpart_sources().to_vec(),
                dispute.person_position().to_owned(),
                dispute.person_evidence().to_vec(),
                dispute.review_rationale().map(str::to_owned),
                dispute.review_evidence().to_vec(),
                dispute.state(),
                cost,
            ),
        ));
    }

    for candidate in result.candidates() {
        if used_tokens >= budget.get() {
            break;
        }
        if selected_refs.contains(&candidate.reference()) {
            continue;
        }
        match candidate.authority() {
            AuthoritativeCandidate::Evidence(primary) => {
                let neighbors = repository
                    .recall_neighbors(candidate.reference(), query.source_scope())
                    .map_err(FreezeFailure::Repository)?;
                append_evidence_windows(
                    repository,
                    primary,
                    neighbors,
                    query,
                    budget,
                    &mut used_tokens,
                    &mut selected_refs,
                    &mut items,
                )?;
            }
            AuthoritativeCandidate::Ledger(claim) => {
                let cost = estimate_claim_tokens(claim);
                if used_tokens.saturating_add(cost) <= budget.get() {
                    used_tokens += cost;
                    selected_refs.insert(candidate.reference());
                    items.push(RetrievedContextItem::LedgerClaim(FrozenLedgerClaim::new(
                        claim.clone(),
                        cost,
                    )));
                }
            }
        }
    }

    let replay_digest = replay_digest(
        query,
        budget,
        &selected_conversation,
        &items,
        used_tokens,
        frozen_at,
    );
    let snapshot = RetrievalSnapshot::new(
        RETRIEVAL_INDEX_VERSION,
        EMBEDDING_MODEL_VERSION,
        budget.get(),
        used_tokens,
        replay_digest,
    );
    let context = WorkingContext::from_selected_evidence(selected_conversation, frozen_at)
        .with_retrieval(items, snapshot)
        .map_err(FreezeFailure::WorkingContext)?;
    Ok(context.with_decision_impact(query.decision_impact()))
}

#[allow(clippy::too_many_arguments)]
fn append_evidence_windows<R: RetrievalRepository>(
    repository: &R,
    primary: &AuthoritativeEvidence,
    mut neighbor_hits: Vec<RecallHit>,
    query: &RetrievalQuery,
    budget: TokenBudget,
    used_tokens: &mut usize,
    selected_refs: &mut BTreeSet<CandidateRef>,
    items: &mut Vec<RetrievedContextItem>,
) -> Result<(), FreezeFailure<R::Error>> {
    neighbor_hits.sort_by_key(|hit| {
        (
            std::cmp::Reverse(hit.channels().count()),
            std::cmp::Reverse(hit.lexical_score()),
            std::cmp::Reverse(hit.vector_score_bps()),
            hit.reference(),
        )
    });
    let seed_reference = CandidateRef::evidence(primary.view().reference());
    let mut same_revision = vec![primary.clone()];
    let mut other_neighbors = Vec::new();
    for hit in neighbor_hits {
        if hit.reference() == seed_reference || selected_refs.contains(&hit.reference()) {
            continue;
        }
        let Some(authority) = repository
            .resolve_authoritative(hit.reference(), query.source_scope())
            .map_err(FreezeFailure::Repository)?
        else {
            continue;
        };
        let AuthoritativeCandidate::Evidence(evidence) = authority else {
            continue;
        };
        if evidence.view().reference().evidence_id() == primary.view().reference().evidence_id() {
            if evidence
                .view()
                .block()
                .ordinal()
                .abs_diff(primary.view().block().ordinal())
                <= 1
            {
                same_revision.push(evidence);
            }
        } else {
            other_neighbors.push(evidence);
        }
    }

    append_one_window(
        primary,
        same_revision,
        budget,
        used_tokens,
        selected_refs,
        items,
    );
    for neighbor in other_neighbors {
        if *used_tokens >= budget.get() {
            break;
        }
        append_one_window(
            &neighbor,
            vec![neighbor.clone()],
            budget,
            used_tokens,
            selected_refs,
            items,
        );
    }
    Ok(())
}

fn append_one_window(
    primary: &AuthoritativeEvidence,
    mut candidates: Vec<AuthoritativeEvidence>,
    budget: TokenBudget,
    used_tokens: &mut usize,
    selected_refs: &mut BTreeSet<CandidateRef>,
    items: &mut Vec<RetrievedContextItem>,
) {
    let seed_ordinal = primary.view().block().ordinal();
    let seed_reference = primary.view().reference();
    candidates.sort_by_key(|evidence| {
        let ordinal = evidence.view().block().ordinal();
        (ordinal.abs_diff(seed_ordinal), ordinal)
    });
    candidates.dedup_by_key(|evidence| evidence.view().reference());

    let mut selected = vec![freeze_evidence(primary)];
    let mut selected_cost = estimate_window_tokens(&selected);
    if used_tokens.saturating_add(selected_cost) > budget.get() {
        return;
    }
    for evidence in candidates {
        let reference = CandidateRef::evidence(evidence.view().reference());
        if evidence.view().reference() == seed_reference || selected_refs.contains(&reference) {
            continue;
        }
        let block = freeze_evidence(&evidence);
        let mut proposed = selected.clone();
        proposed.push(block);
        proposed.sort_by_key(FrozenEvidenceBlock::ordinal);
        let proposed_cost = estimate_window_tokens(&proposed);
        if used_tokens.saturating_add(proposed_cost) <= budget.get() {
            selected = proposed;
            selected_cost = proposed_cost;
        }
    }
    for block in &selected {
        selected_refs.insert(CandidateRef::Evidence {
            evidence_id: block.evidence_id(),
            block_id: block.block_id(),
        });
    }
    *used_tokens += selected_cost;
    let window_ordinal = items
        .iter()
        .filter(|item| matches!(item, RetrievedContextItem::EvidenceWindow(_)))
        .count();
    items.push(RetrievedContextItem::EvidenceWindow(
        FrozenRetrievalWindow::new(window_ordinal, selected, selected_cost),
    ));
}

fn freeze_evidence(evidence: &AuthoritativeEvidence) -> FrozenEvidenceBlock {
    FrozenEvidenceBlock::new(
        evidence.view().reference().evidence_id(),
        evidence.view().reference().block_id().get(),
        evidence.view().block().ordinal(),
        evidence.view().verbatim().to_owned(),
        evidence.source_record_id(),
        evidence.source_locator().to_owned(),
        match evidence.currentness() {
            SourceCurrentness::Present => CoreSourceCurrentness::Present,
            SourceCurrentness::SourceRemoved => CoreSourceCurrentness::SourceRemoved,
        },
        Timestamp::from_millis(evidence.recorded_at_millis()),
    )
}

fn estimate_window_tokens(blocks: &[FrozenEvidenceBlock]) -> usize {
    WINDOW_METADATA_TOKENS
        + blocks
            .iter()
            .map(|block| {
                BLOCK_METADATA_TOKENS
                    + estimate_text_tokens(block.verbatim())
                    + estimate_text_tokens(block.source_locator())
            })
            .sum::<usize>()
}

fn estimate_claim_tokens(claim: &eam_core::Claim) -> usize {
    CLAIM_METADATA_TOKENS
        + estimate_text_tokens(claim.statement())
        + claim
            .support()
            .iter()
            .map(|citation| estimate_text_tokens(citation.quote()).saturating_add(4))
            .sum::<usize>()
}

fn estimate_dispute_tokens(dispute: &DisputedMemoryRecall) -> usize {
    DISPUTE_METADATA_TOKENS
        + estimate_text_tokens(dispute.counterpart_view())
        + dispute
            .counterpart_sources()
            .iter()
            .map(estimate_claim_tokens)
            .sum::<usize>()
        + estimate_text_tokens(dispute.person_position())
        + dispute
            .person_evidence()
            .iter()
            .map(|citation| estimate_text_tokens(citation.quote()).saturating_add(4))
            .sum::<usize>()
        + dispute.review_rationale().map_or(0, estimate_text_tokens)
        + dispute
            .review_evidence()
            .iter()
            .map(|citation| estimate_text_tokens(citation.quote()).saturating_add(4))
            .sum::<usize>()
}

fn estimate_text_tokens(value: &str) -> usize {
    let mut ascii_bytes = 0_usize;
    let mut non_ascii = 0_usize;
    for character in value.chars() {
        if character.is_ascii() {
            ascii_bytes += 1;
        } else {
            non_ascii += 1;
        }
    }
    ascii_bytes.div_ceil(4).saturating_add(non_ascii)
}

fn replay_digest(
    query: &RetrievalQuery,
    budget: TokenBudget,
    conversation: &[ConversationEvidence],
    items: &[RetrievedContextItem],
    used_tokens: usize,
    frozen_at: Timestamp,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, RETRIEVAL_INDEX_VERSION.as_bytes());
    hash_bytes(&mut hasher, EMBEDDING_MODEL_VERSION.as_bytes());
    hash_optional_bytes(&mut hasher, query.text().map(str::as_bytes));
    hash_optional_i64(
        &mut hasher,
        query.time().map(crate::TimeRange::start_millis),
    );
    hash_optional_i64(&mut hasher, query.time().map(crate::TimeRange::end_millis));
    hasher.update([match query.source_scope() {
        crate::SourceScope::Current => 0,
        crate::SourceScope::Historical => 1,
    }]);
    hasher.update([match query.decision_impact() {
        eam_core::DecisionImpact::Ordinary => 0,
        eam_core::DecisionImpact::High => 1,
    }]);
    hash_usize(&mut hasher, query.limit());
    for entity in query.entities() {
        hash_bytes(&mut hasher, entity.as_bytes());
    }
    hash_usize(&mut hasher, budget.get());
    hash_usize(&mut hasher, used_tokens);
    hash_i64(&mut hasher, frozen_at.as_millis());
    for evidence in conversation {
        hash_u64(&mut hasher, evidence.id().get());
        hash_bytes(&mut hasher, evidence.verbatim().as_bytes());
        hash_i64(&mut hasher, evidence.recorded_at().as_millis());
    }
    for item in items {
        match item {
            RetrievedContextItem::EvidenceWindow(window) => {
                hasher.update([0]);
                hash_usize(&mut hasher, window.ordinal());
                for block in window.blocks() {
                    hash_u64(&mut hasher, block.evidence_id());
                    hash_u64(&mut hasher, block.block_id());
                    hash_usize(&mut hasher, block.ordinal());
                    hash_bytes(&mut hasher, block.verbatim().as_bytes());
                    hash_u64(&mut hasher, block.source_record_id());
                    hash_bytes(&mut hasher, block.source_locator().as_bytes());
                    hasher.update([match block.currentness() {
                        CoreSourceCurrentness::Present => 0,
                        CoreSourceCurrentness::SourceRemoved => 1,
                    }]);
                    hash_i64(&mut hasher, block.recorded_at().as_millis());
                }
            }
            RetrievedContextItem::LedgerClaim(frozen) => {
                hasher.update([1]);
                let claim = frozen.claim();
                hash_u64(&mut hasher, claim.id().get());
                hash_bytes(&mut hasher, claim.statement().as_bytes());
                hash_applicable_time(&mut hasher, claim.applicable_time());
                hash_i64(&mut hasher, claim.recorded_at().as_millis());
                for citation in claim.support() {
                    hash_u64(&mut hasher, citation.evidence_id().get());
                    hash_bytes(&mut hasher, citation.quote().as_bytes());
                }
            }
            RetrievedContextItem::MemoryDispute(dispute) => {
                hasher.update([2]);
                hash_u64(&mut hasher, dispute.dispute_id());
                hash_u64(&mut hasher, dispute.memory_id());
                hash_u64(&mut hasher, dispute.memory_version());
                hash_bytes(&mut hasher, dispute.counterpart_view().as_bytes());
                for claim in dispute.counterpart_sources() {
                    hash_u64(&mut hasher, claim.id().get());
                    hash_bytes(&mut hasher, claim.statement().as_bytes());
                    for citation in claim.support() {
                        hash_u64(&mut hasher, citation.evidence_id().get());
                        hash_bytes(&mut hasher, citation.quote().as_bytes());
                    }
                }
                hash_bytes(&mut hasher, dispute.person_position().as_bytes());
                for citation in dispute.person_evidence() {
                    hash_u64(&mut hasher, citation.evidence_id().get());
                    hash_bytes(&mut hasher, citation.quote().as_bytes());
                }
                hash_optional_bytes(&mut hasher, dispute.review_rationale().map(str::as_bytes));
                for citation in dispute.review_evidence() {
                    hash_u64(&mut hasher, citation.evidence_id().get());
                    hash_bytes(&mut hasher, citation.quote().as_bytes());
                }
                hasher.update([match dispute.state() {
                    eam_core::DisputeState::Open => 0,
                    eam_core::DisputeState::Maintained => 1,
                }]);
            }
        }
    }
    hasher.finalize().into()
}

fn hash_applicable_time(hasher: &mut Sha256, value: ApplicableTime) {
    match value {
        ApplicableTime::At(at) => {
            hasher.update([0]);
            hash_i64(hasher, at.as_millis());
        }
        ApplicableTime::Since(since) => {
            hasher.update([1]);
            hash_i64(hasher, since.as_millis());
        }
        ApplicableTime::Between { start, end } => {
            hasher.update([2]);
            hash_i64(hasher, start.as_millis());
            hash_i64(hasher, end.as_millis());
        }
        ApplicableTime::Unknown => hasher.update([3]),
    }
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) {
    hash_usize(hasher, value.len());
    hasher.update(value);
}

fn hash_optional_bytes(hasher: &mut Sha256, value: Option<&[u8]>) {
    hasher.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        hash_bytes(hasher, value);
    }
}

fn hash_optional_i64(hasher: &mut Sha256, value: Option<i64>) {
    hasher.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        hash_i64(hasher, value);
    }
}

fn hash_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_le_bytes());
}

fn hash_usize(hasher: &mut Sha256, value: usize) {
    hash_u64(hasher, u64::try_from(value).unwrap_or(u64::MAX));
}

fn hash_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_le_bytes());
}

#[derive(Debug)]
pub enum FreezeFailure<E> {
    Retrieval(RetrievalFailure<E>),
    Repository(E),
    WorkingContext(WorkingContextError),
}

impl<E: fmt::Display> fmt::Display for FreezeFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retrieval(error) => {
                write!(formatter, "working-context retrieval failed: {error}")
            }
            Self::Repository(error) => write!(formatter, "neighbor retrieval failed: {error}"),
            Self::WorkingContext(error) => write!(formatter, "working context rejected: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for FreezeFailure<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Retrieval(error) => Some(error),
            Self::Repository(error) => Some(error),
            Self::WorkingContext(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_budget_rejects_values_outside_the_frozen_range() {
        assert_eq!(
            TokenBudget::new(MIN_TOKEN_BUDGET - 1),
            Err(crate::RetrievalError::InvalidTokenBudget)
        );
        assert_eq!(
            TokenBudget::new(MAX_TOKEN_BUDGET + 1),
            Err(crate::RetrievalError::InvalidTokenBudget)
        );
        assert_eq!(TokenBudget::default().get(), DEFAULT_TOKEN_BUDGET);
    }
}
