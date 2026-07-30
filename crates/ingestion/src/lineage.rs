use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use eam_markdown::MarkdownBlockKind;

use crate::{
    EvidenceBlock, EvidenceBlockRef, EvidenceError, ExtractionRevisionId, MarkdownLocatorValue,
    MaterializedExtraction,
};

pub const BLOCK_LINEAGE_RULE_VERSION: &str = "eam-block-lineage-v1";
const MODIFIED_SCORE_THRESHOLD_BP: u16 = 7_000;
const MODIFIED_WINNER_MARGIN_BP: u16 = 1_500;
const MODIFIED_ORDINAL_WINDOW: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockLineageStatus {
    Unchanged,
    Moved,
    Modified,
    Removed,
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineageBasis {
    UniqueNativeLocator,
    UniqueExactFingerprint,
    ModifiedSimilarity { score_basis_points: u16 },
    NoCandidate,
    AmbiguousCandidates { candidates: Vec<EvidenceBlockRef> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockLineage {
    from_ref: EvidenceBlockRef,
    to_ref: Option<EvidenceBlockRef>,
    status: BlockLineageStatus,
    basis: LineageBasis,
}

impl BlockLineage {
    /// Restores one validated persisted lineage edge.
    ///
    /// # Errors
    ///
    /// Rejects status, target, and basis combinations that could imply an
    /// automatic projection for modified, removed, or ambiguous content.
    pub fn new(
        from_ref: EvidenceBlockRef,
        to_ref: Option<EvidenceBlockRef>,
        status: BlockLineageStatus,
        basis: LineageBasis,
    ) -> Result<Self, LineageError> {
        let valid = match status {
            BlockLineageStatus::Unchanged | BlockLineageStatus::Moved => {
                to_ref.is_some()
                    && matches!(
                        &basis,
                        LineageBasis::UniqueNativeLocator | LineageBasis::UniqueExactFingerprint
                    )
            }
            BlockLineageStatus::Modified => {
                to_ref.is_some()
                    && match &basis {
                        LineageBasis::UniqueNativeLocator => true,
                        LineageBasis::ModifiedSimilarity { score_basis_points } => {
                            *score_basis_points >= MODIFIED_SCORE_THRESHOLD_BP
                        }
                        _ => false,
                    }
            }
            BlockLineageStatus::Removed => {
                to_ref.is_none() && matches!(&basis, LineageBasis::NoCandidate)
            }
            BlockLineageStatus::Ambiguous => {
                to_ref.is_none()
                    && matches!(
                        &basis,
                        LineageBasis::AmbiguousCandidates { candidates }
                            if !candidates.is_empty()
                    )
            }
        };
        if !valid {
            return Err(LineageError::InvalidStoredLineage);
        }
        Ok(Self {
            from_ref,
            to_ref,
            status,
            basis,
        })
    }

    #[must_use]
    pub const fn from_ref(&self) -> EvidenceBlockRef {
        self.from_ref
    }

    #[must_use]
    pub const fn to_ref(&self) -> Option<EvidenceBlockRef> {
        self.to_ref
    }

    #[must_use]
    pub const fn status(&self) -> BlockLineageStatus {
        self.status
    }

    #[must_use]
    pub const fn basis(&self) -> &LineageBasis {
        &self.basis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncrementalWorkItem {
    AdvanceCurrentProjection {
        from_ref: EvidenceBlockRef,
        to_ref: EvidenceBlockRef,
    },
    ReuseIndexPayload {
        from_ref: EvidenceBlockRef,
        to_ref: EvidenceBlockRef,
    },
    RebuildIndex {
        to_ref: EvidenceBlockRef,
    },
    ReviewMemory {
        from_ref: EvidenceBlockRef,
        reason: BlockLineageStatus,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalWorkPlan {
    items: Vec<IncrementalWorkItem>,
}

impl IncrementalWorkPlan {
    #[must_use]
    pub const fn new(items: Vec<IncrementalWorkItem>) -> Self {
        Self { items }
    }

    #[must_use]
    pub fn items(&self) -> &[IncrementalWorkItem] {
        &self.items
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineageBatch {
    source_record_id: u64,
    from_revision_id: ExtractionRevisionId,
    to_revision_id: ExtractionRevisionId,
    decided_at_millis: i64,
    rule_version: String,
    lineages: Vec<BlockLineage>,
    work_plan: IncrementalWorkPlan,
}

impl LineageBatch {
    /// Restores one immutable lineage batch and its persisted work plan.
    ///
    /// # Errors
    ///
    /// Rejects invalid identifiers, revision pairs, rule versions, duplicate
    /// predecessor references, or lineages owned by another revision pair.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_record_id: u64,
        from_revision_id: ExtractionRevisionId,
        to_revision_id: ExtractionRevisionId,
        decided_at_millis: i64,
        rule_version: String,
        lineages: Vec<BlockLineage>,
        work_plan: IncrementalWorkPlan,
    ) -> Result<Self, LineageError> {
        if source_record_id == 0 {
            return Err(LineageError::InvalidSourceRecord);
        }
        if from_revision_id == to_revision_id || rule_version.trim().is_empty() {
            return Err(LineageError::InvalidRevisionPair);
        }
        let unique_from = lineages
            .iter()
            .map(BlockLineage::from_ref)
            .collect::<HashSet<_>>();
        if unique_from.len() != lineages.len() {
            return Err(LineageError::InvalidStoredLineage);
        }
        Ok(Self {
            source_record_id,
            from_revision_id,
            to_revision_id,
            decided_at_millis,
            rule_version,
            lineages,
            work_plan,
        })
    }

    #[must_use]
    pub const fn source_record_id(&self) -> u64 {
        self.source_record_id
    }

    #[must_use]
    pub const fn from_revision_id(&self) -> ExtractionRevisionId {
        self.from_revision_id
    }

    #[must_use]
    pub const fn to_revision_id(&self) -> ExtractionRevisionId {
        self.to_revision_id
    }

    #[must_use]
    pub const fn decided_at_millis(&self) -> i64 {
        self.decided_at_millis
    }

    #[must_use]
    pub fn rule_version(&self) -> &str {
        &self.rule_version
    }

    #[must_use]
    pub fn lineages(&self) -> &[BlockLineage] {
        &self.lineages
    }

    #[must_use]
    pub const fn work_plan(&self) -> &IncrementalWorkPlan {
        &self.work_plan
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalLineageRevision {
    extraction: MaterializedExtraction,
    canonical_text: String,
}

impl CanonicalLineageRevision {
    /// Couples one immutable extraction with its authenticated canonical text.
    ///
    /// # Errors
    ///
    /// Rejects a text that cannot reproduce every persisted block quote.
    pub fn new(
        extraction: MaterializedExtraction,
        canonical_text: String,
    ) -> Result<Self, LineageError> {
        for block in extraction.blocks() {
            block
                .anchor()
                .quote(&canonical_text)
                .map_err(LineageError::InvalidCanonicalBlock)?;
        }
        Ok(Self {
            extraction,
            canonical_text,
        })
    }

    #[must_use]
    pub const fn extraction(&self) -> &MaterializedExtraction {
        &self.extraction
    }

    #[must_use]
    pub fn canonical_text(&self) -> &str {
        &self.canonical_text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineagePair {
    source_record_id: u64,
    previous: CanonicalLineageRevision,
    current: CanonicalLineageRevision,
}

impl LineagePair {
    /// Creates the adjacent revision input loaded by trusted storage.
    ///
    /// # Errors
    ///
    /// Rejects zero source identity or the same extraction on both sides.
    pub fn new(
        source_record_id: u64,
        previous: CanonicalLineageRevision,
        current: CanonicalLineageRevision,
    ) -> Result<Self, LineageError> {
        if source_record_id == 0 {
            return Err(LineageError::InvalidSourceRecord);
        }
        if previous.extraction().revision().id() == current.extraction().revision().id() {
            return Err(LineageError::InvalidRevisionPair);
        }
        Ok(Self {
            source_record_id,
            previous,
            current,
        })
    }

    #[must_use]
    pub const fn source_record_id(&self) -> u64 {
        self.source_record_id
    }

    #[must_use]
    pub const fn previous(&self) -> &CanonicalLineageRevision {
        &self.previous
    }

    #[must_use]
    pub const fn current(&self) -> &CanonicalLineageRevision {
        &self.current
    }
}

pub trait BlockLineageRepository {
    type Error;

    /// Loads the current extraction and its immediately preceding extraction
    /// for the same stable source record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error for missing, corrupt, or unauthenticated state.
    fn load_lineage_pair(
        &self,
        to_revision_id: ExtractionRevisionId,
    ) -> Result<Option<LineagePair>, Self::Error>;

    /// Atomically persists all edges, ambiguity candidates, and work items.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without leaving a partial lineage batch.
    fn commit_lineage_batch(&mut self, batch: &LineageBatch) -> Result<LineageBatch, Self::Error>;

    /// Restores a previously committed batch by target revision and rule.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when persisted lineage state is invalid.
    fn load_lineage_batch(
        &self,
        to_revision_id: ExtractionRevisionId,
        rule_version: &str,
    ) -> Result<Option<LineageBatch>, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineageError {
    InvalidSourceRecord,
    InvalidRevisionPair,
    InvalidCanonicalBlock(EvidenceError),
    InvalidStoredLineage,
}

impl fmt::Display for LineageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceRecord => {
                formatter.write_str("source record identifier must be positive")
            }
            Self::InvalidRevisionPair => formatter.write_str("lineage revisions must be distinct"),
            Self::InvalidCanonicalBlock(error) => {
                write!(formatter, "lineage input is invalid: {error}")
            }
            Self::InvalidStoredLineage => {
                formatter.write_str("persisted block lineage is structurally invalid")
            }
        }
    }
}

impl Error for LineageError {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ExactKey {
    kind: u8,
    heading_level: Option<u8>,
    list_start: Option<u64>,
    task_checked: Option<bool>,
    info_string: Option<String>,
    quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NativeKey {
    version: String,
    kind: u8,
    value_kind: u8,
    value: String,
}

struct Snapshot<'a> {
    block: &'a EvidenceBlock,
    quote: &'a str,
    parent_index: Option<usize>,
    exact_key: ExactKey,
    native_key: Option<NativeKey>,
}

#[derive(Clone, Copy)]
enum MatchedBasis {
    Native,
    Exact,
    Modified(u16),
}

/// Computes explicit block continuity and the deterministic downstream work plan.
///
/// # Errors
///
/// Rejects a zero source record, the same revision on both sides, or a block
/// whose persisted source anchor does not slice the supplied canonical text.
pub fn compute_block_lineage(
    source_record_id: u64,
    previous: &MaterializedExtraction,
    previous_canonical_text: &str,
    current: &MaterializedExtraction,
    current_canonical_text: &str,
    decided_at_millis: i64,
) -> Result<LineageBatch, LineageError> {
    if source_record_id == 0 {
        return Err(LineageError::InvalidSourceRecord);
    }
    if previous.revision().id() == current.revision().id() {
        return Err(LineageError::InvalidRevisionPair);
    }

    let previous_snapshots = snapshots(previous, previous_canonical_text)?;
    let current_snapshots = snapshots(current, current_canonical_text)?;
    let mut matched_old = vec![None; previous_snapshots.len()];
    let mut matched_new = vec![None; current_snapshots.len()];
    let mut contested_old = HashMap::<usize, HashSet<usize>>::new();
    let mut contested_new = HashSet::<usize>::new();

    match_unique_keys(
        &previous_snapshots,
        &current_snapshots,
        |snapshot| snapshot.native_key.as_ref(),
        MatchedBasis::Native,
        &mut matched_old,
        &mut matched_new,
        &mut contested_old,
        &mut contested_new,
    );
    match_unique_keys(
        &previous_snapshots,
        &current_snapshots,
        |snapshot| Some(&snapshot.exact_key),
        MatchedBasis::Exact,
        &mut matched_old,
        &mut matched_new,
        &mut contested_old,
        &mut contested_new,
    );

    match_modified_candidates(
        &previous_snapshots,
        &current_snapshots,
        &mut matched_old,
        &mut matched_new,
        &mut contested_old,
        &contested_new,
    );

    let lineages = assemble_lineages(
        &previous_snapshots,
        &current_snapshots,
        &matched_old,
        &contested_old,
    );
    let work_plan = build_work_plan(&lineages, current.blocks());
    Ok(LineageBatch {
        source_record_id,
        from_revision_id: previous.revision().id(),
        to_revision_id: current.revision().id(),
        decided_at_millis,
        rule_version: BLOCK_LINEAGE_RULE_VERSION.to_owned(),
        lineages,
        work_plan,
    })
}

fn assemble_lineages(
    previous: &[Snapshot<'_>],
    current: &[Snapshot<'_>],
    matched_old: &[Option<(usize, MatchedBasis)>],
    contested_old: &HashMap<usize, HashSet<usize>>,
) -> Vec<BlockLineage> {
    let matches = matched_old
        .iter()
        .enumerate()
        .filter_map(|(old_index, matched)| matched.map(|(new_index, _)| (old_index, new_index)))
        .collect::<HashMap<_, _>>();
    let mut lineages = Vec::with_capacity(previous.len());
    for (old_index, old) in previous.iter().enumerate() {
        let lineage = if let Some((new_index, basis)) = matched_old[old_index] {
            let current_block = current[new_index].block;
            match basis {
                MatchedBasis::Modified(score_basis_points) => BlockLineage {
                    from_ref: old.block.reference(),
                    to_ref: Some(current_block.reference()),
                    status: BlockLineageStatus::Modified,
                    basis: LineageBasis::ModifiedSimilarity { score_basis_points },
                },
                MatchedBasis::Native if old.exact_key != current[new_index].exact_key => {
                    BlockLineage {
                        from_ref: old.block.reference(),
                        to_ref: Some(current_block.reference()),
                        status: BlockLineageStatus::Modified,
                        basis: LineageBasis::UniqueNativeLocator,
                    }
                }
                MatchedBasis::Native | MatchedBasis::Exact => {
                    let status = if same_structural_path(
                        old_index, new_index, previous, current, &matches,
                    ) {
                        BlockLineageStatus::Unchanged
                    } else {
                        BlockLineageStatus::Moved
                    };
                    BlockLineage {
                        from_ref: old.block.reference(),
                        to_ref: Some(current_block.reference()),
                        status,
                        basis: match basis {
                            MatchedBasis::Native => LineageBasis::UniqueNativeLocator,
                            MatchedBasis::Exact => LineageBasis::UniqueExactFingerprint,
                            MatchedBasis::Modified(_) => unreachable!(),
                        },
                    }
                }
            }
        } else if let Some(candidates) = contested_old.get(&old_index) {
            let mut candidates = candidates
                .iter()
                .map(|index| current[*index].block.reference())
                .collect::<Vec<_>>();
            candidates.sort_by_key(|reference| reference.block_id().get());
            candidates.dedup();
            BlockLineage {
                from_ref: old.block.reference(),
                to_ref: None,
                status: BlockLineageStatus::Ambiguous,
                basis: LineageBasis::AmbiguousCandidates { candidates },
            }
        } else {
            BlockLineage {
                from_ref: old.block.reference(),
                to_ref: None,
                status: BlockLineageStatus::Removed,
                basis: LineageBasis::NoCandidate,
            }
        };
        lineages.push(lineage);
    }
    lineages
}

fn snapshots<'a>(
    extraction: &'a MaterializedExtraction,
    canonical_text: &'a str,
) -> Result<Vec<Snapshot<'a>>, LineageError> {
    let by_id = extraction
        .blocks()
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id(), index))
        .collect::<HashMap<_, _>>();
    extraction
        .blocks()
        .iter()
        .map(|block| {
            let quote = block
                .anchor()
                .quote(canonical_text)
                .map_err(LineageError::InvalidCanonicalBlock)?;
            let metadata = block.metadata();
            Ok(Snapshot {
                block,
                quote,
                parent_index: block
                    .parent_id()
                    .and_then(|parent| by_id.get(&parent).copied()),
                exact_key: ExactKey {
                    kind: kind_code(block.kind()),
                    heading_level: metadata.heading_level(),
                    list_start: metadata.list_start(),
                    task_checked: metadata.task_checked(),
                    info_string: metadata.info_string().map(str::to_owned),
                    quote: quote.to_owned(),
                },
                native_key: block.anchor().native_locator().map(|locator| {
                    let (value_kind, value) = match locator.value() {
                        MarkdownLocatorValue::Heading { text } => (0, text.clone()),
                        MarkdownLocatorValue::BlockId { id } => (1, id.clone()),
                    };
                    NativeKey {
                        version: locator.version().to_owned(),
                        kind: kind_code(block.kind()),
                        value_kind,
                        value,
                    }
                }),
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn match_unique_keys<'a, K: Eq + std::hash::Hash + ?Sized + 'a>(
    previous: &'a [Snapshot<'a>],
    current: &'a [Snapshot<'a>],
    key: impl Fn(&'a Snapshot<'a>) -> Option<&'a K>,
    basis: MatchedBasis,
    matched_old: &mut [Option<(usize, MatchedBasis)>],
    matched_new: &mut [Option<usize>],
    contested_old: &mut HashMap<usize, HashSet<usize>>,
    contested_new: &mut HashSet<usize>,
) {
    let mut old_groups = HashMap::<&K, Vec<usize>>::new();
    let mut new_groups = HashMap::<&K, Vec<usize>>::new();
    for (index, snapshot) in previous.iter().enumerate() {
        if matched_old[index].is_none()
            && let Some(key) = key(snapshot)
        {
            old_groups.entry(key).or_default().push(index);
        }
    }
    for (index, snapshot) in current.iter().enumerate() {
        if matched_new[index].is_none()
            && let Some(key) = key(snapshot)
        {
            new_groups.entry(key).or_default().push(index);
        }
    }
    for (key, old_indices) in old_groups {
        let Some(new_indices) = new_groups.get(key) else {
            continue;
        };
        if old_indices.len() == 1 && new_indices.len() == 1 {
            let old_index = old_indices[0];
            let new_index = new_indices[0];
            matched_old[old_index] = Some((new_index, basis));
            matched_new[new_index] = Some(old_index);
        } else {
            for old_index in old_indices {
                contested_old
                    .entry(old_index)
                    .or_default()
                    .extend(new_indices.iter().copied());
            }
            contested_new.extend(new_indices.iter().copied());
        }
    }
}

fn match_modified_candidates(
    previous: &[Snapshot<'_>],
    current: &[Snapshot<'_>],
    matched_old: &mut [Option<(usize, MatchedBasis)>],
    matched_new: &mut [Option<usize>],
    contested_old: &mut HashMap<usize, HashSet<usize>>,
    contested_new: &HashSet<usize>,
) {
    let exact_parent_matches = matched_old
        .iter()
        .enumerate()
        .filter_map(|(old, matched)| matched.map(|(new, _)| (old, new)))
        .collect::<HashMap<_, _>>();
    let mut candidates = HashMap::<usize, Vec<(usize, u16)>>::new();
    for (old_index, old) in previous.iter().enumerate() {
        if matched_old[old_index].is_some() || contested_old.contains_key(&old_index) {
            continue;
        }
        for (new_index, new) in current.iter().enumerate() {
            if matched_new[new_index].is_some()
                || contested_new.contains(&new_index)
                || old.block.kind() != new.block.kind()
                || old.block.ordinal().abs_diff(new.block.ordinal()) > MODIFIED_ORDINAL_WINDOW
                || !parents_compatible(old, new, &exact_parent_matches)
            {
                continue;
            }
            let Some(score) = trigram_dice_basis_points(old.quote, new.quote) else {
                continue;
            };
            if score >= MODIFIED_SCORE_THRESHOLD_BP {
                candidates
                    .entry(old_index)
                    .or_default()
                    .push((new_index, score));
            }
        }
    }

    let mut best_new_for_old = HashMap::<usize, (usize, u16, bool)>::new();
    let mut old_candidates_for_new = HashMap::<usize, Vec<(usize, u16)>>::new();
    for (&old_index, values) in &candidates {
        for &(new_index, score) in values {
            old_candidates_for_new
                .entry(new_index)
                .or_default()
                .push((old_index, score));
        }
        if let Some((new_index, score, unique_margin)) = unique_winner(values) {
            best_new_for_old.insert(old_index, (new_index, score, unique_margin));
        }
    }

    for (&old_index, &(new_index, score, old_margin)) in &best_new_for_old {
        let reciprocal = old_candidates_for_new
            .get(&new_index)
            .and_then(|values| unique_winner(values));
        if old_margin
            && reciprocal
                .is_some_and(|(best_old, _, new_margin)| best_old == old_index && new_margin)
        {
            matched_old[old_index] = Some((new_index, MatchedBasis::Modified(score)));
            matched_new[new_index] = Some(old_index);
        }
    }

    for (old_index, values) in candidates {
        if matched_old[old_index].is_none() {
            contested_old
                .entry(old_index)
                .or_default()
                .extend(values.into_iter().map(|(new_index, _)| new_index));
        }
    }
}

fn unique_winner(values: &[(usize, u16)]) -> Option<(usize, u16, bool)> {
    let mut ranked = values.to_vec();
    ranked.sort_by_key(|(index, score)| (std::cmp::Reverse(*score), *index));
    let &(index, score) = ranked.first()?;
    let runner_up = ranked.get(1).map_or(0, |(_, score)| *score);
    Some((
        index,
        score,
        score.saturating_sub(runner_up) >= MODIFIED_WINNER_MARGIN_BP,
    ))
}

fn parents_compatible(
    old: &Snapshot<'_>,
    new: &Snapshot<'_>,
    matches: &HashMap<usize, usize>,
) -> bool {
    match (old.parent_index, new.parent_index) {
        (None, None) => true,
        (Some(old_parent), Some(new_parent)) => matches.get(&old_parent) == Some(&new_parent),
        _ => false,
    }
}

fn same_structural_path(
    mut old_index: usize,
    mut new_index: usize,
    previous: &[Snapshot<'_>],
    current: &[Snapshot<'_>],
    matches: &HashMap<usize, usize>,
) -> bool {
    loop {
        if previous[old_index].block.ordinal() != current[new_index].block.ordinal() {
            return false;
        }
        match (
            previous[old_index].parent_index,
            current[new_index].parent_index,
        ) {
            (None, None) => return true,
            (Some(old_parent), Some(new_parent))
                if matches.get(&old_parent) == Some(&new_parent) =>
            {
                old_index = old_parent;
                new_index = new_parent;
            }
            _ => return false,
        }
    }
}

fn build_work_plan(
    lineages: &[BlockLineage],
    current_blocks: &[EvidenceBlock],
) -> IncrementalWorkPlan {
    let mut items = Vec::new();
    let mut reusable_current = HashSet::new();
    for lineage in lineages {
        if matches!(
            lineage.status,
            BlockLineageStatus::Unchanged | BlockLineageStatus::Moved
        ) {
            let to_ref = lineage
                .to_ref
                .expect("projectable lineage always has a current reference");
            reusable_current.insert(to_ref);
            items.push(IncrementalWorkItem::AdvanceCurrentProjection {
                from_ref: lineage.from_ref,
                to_ref,
            });
            items.push(IncrementalWorkItem::ReuseIndexPayload {
                from_ref: lineage.from_ref,
                to_ref,
            });
        }
    }
    for block in current_blocks {
        let to_ref = block.reference();
        if !reusable_current.contains(&to_ref) {
            items.push(IncrementalWorkItem::RebuildIndex { to_ref });
        }
    }
    for lineage in lineages {
        if matches!(
            lineage.status,
            BlockLineageStatus::Modified
                | BlockLineageStatus::Removed
                | BlockLineageStatus::Ambiguous
        ) {
            items.push(IncrementalWorkItem::ReviewMemory {
                from_ref: lineage.from_ref,
                reason: lineage.status,
            });
        }
    }
    IncrementalWorkPlan { items }
}

fn trigram_dice_basis_points(left: &str, right: &str) -> Option<u16> {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len() < 3 || right.len() < 3 {
        return None;
    }
    let left_grams = trigram_counts(&left);
    let right_grams = trigram_counts(&right);
    let left_total = left_grams.values().sum::<usize>();
    let right_total = right_grams.values().sum::<usize>();
    let intersection = left_grams
        .iter()
        .map(|(gram, count)| count.min(right_grams.get(gram).unwrap_or(&0)))
        .sum::<usize>();
    let numerator = 2_u64
        .saturating_mul(u64::try_from(intersection).ok()?)
        .saturating_mul(10_000);
    let denominator = u64::try_from(left_total.checked_add(right_total)?).ok()?;
    u16::try_from(numerator / denominator).ok()
}

fn trigram_counts(chars: &[char]) -> HashMap<[char; 3], usize> {
    let mut counts = HashMap::new();
    for window in chars.windows(3) {
        *counts.entry([window[0], window[1], window[2]]).or_insert(0) += 1;
    }
    counts
}

const fn kind_code(kind: MarkdownBlockKind) -> u8 {
    match kind {
        MarkdownBlockKind::Paragraph => 0,
        MarkdownBlockKind::Heading => 1,
        MarkdownBlockKind::BlockQuote => 2,
        MarkdownBlockKind::List => 3,
        MarkdownBlockKind::ListItem => 4,
        MarkdownBlockKind::CodeBlock => 5,
        MarkdownBlockKind::Table => 6,
        MarkdownBlockKind::TableHead => 7,
        MarkdownBlockKind::TableRow => 8,
        MarkdownBlockKind::TableCell => 9,
        MarkdownBlockKind::HtmlBlock => 10,
        MarkdownBlockKind::ThematicBreak => 11,
        MarkdownBlockKind::MetadataBlock => 12,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigram_score_is_deterministic_and_unicode_aware() {
        assert_eq!(
            trigram_dice_basis_points("日本語 abc", "日本語 abc"),
            Some(10_000)
        );
        assert!(trigram_dice_basis_points("日本語 abc", "日本語 abd").unwrap() >= 7_000);
        assert_eq!(trigram_dice_basis_points("ab", "ac"), None);
    }
}
