use std::collections::HashMap;

use eam_ingestion::{
    BlockLineageStatus, EvidenceBlock, EvidenceBlockId, ExtractionRevision, ExtractionRevisionId,
    IncrementalWorkItem, LineageBasis, MaterializedExtraction, compute_block_lineage,
    validate_accepted_markdown,
};
use eam_markdown::{ParseLimits, parse_markdown};

const BASELINE: &str = include_str!("fixtures/lineage/baseline.md");
const INSERTED: &str = include_str!("fixtures/lineage/inserted.md");
const MOVED: &str = include_str!("fixtures/lineage/moved.md");
const MODIFIED: &str = include_str!("fixtures/lineage/modified.md");
const DELETED: &str = include_str!("fixtures/lineage/deleted.md");
const AMBIGUOUS_FROM: &str = include_str!("fixtures/lineage/ambiguous-from.md");
const AMBIGUOUS_TO: &str = include_str!("fixtures/lineage/ambiguous-to.md");
const NATIVE_OLD: &str = "# Native\n\nOriginal stable text. ^stable-block\n";
const NATIVE_NEW: &str = "# Native\n\nCompletely rewritten text. ^stable-block\n";

#[test]
fn insertion_moves_exact_blocks_and_rebuilds_only_the_new_block() {
    let previous = extraction(BASELINE, 1, 1, 1);
    let current = extraction(INSERTED, 2, 2, 100);

    let batch = compute_block_lineage(7, &previous, BASELINE, &current, INSERTED, 50).unwrap();
    assert_eq!(
        status_for_quote(&batch, &previous, BASELINE, "Alpha"),
        BlockLineageStatus::Moved
    );
    assert!(batch.work_plan().items().iter().any(|item| {
        matches!(item, IncrementalWorkItem::RebuildIndex { to_ref }
            if quote_for_ref(&current, INSERTED, *to_ref).contains("Inserted evidence"))
    }));
}

#[test]
fn movement_preserves_history_and_only_advances_exact_continuity() {
    let previous = extraction(BASELINE, 1, 1, 1);
    let current = extraction(MOVED, 2, 2, 100);
    let alpha_ref = ref_for_quote(&previous, BASELINE, "Alpha");

    let batch = compute_block_lineage(7, &previous, BASELINE, &current, MOVED, 50).unwrap();

    assert_eq!(
        status_for_quote(&batch, &previous, BASELINE, "Alpha"),
        BlockLineageStatus::Moved
    );
    assert_eq!(
        status_for_quote(&batch, &previous, BASELINE, "Bravo"),
        BlockLineageStatus::Moved
    );
    assert!(
        batch
            .lineages()
            .iter()
            .any(|lineage| lineage.from_ref() == alpha_ref)
    );
    assert!(batch.work_plan().items().iter().any(|item| {
        matches!(item, IncrementalWorkItem::AdvanceCurrentProjection { from_ref, .. }
            if *from_ref == alpha_ref)
    }));
}

#[test]
fn modification_rebuilds_and_reviews_without_advancing_the_old_reference() {
    let previous = extraction(BASELINE, 1, 1, 1);
    let current = extraction(MODIFIED, 2, 2, 100);
    let bravo_ref = ref_for_quote(&previous, BASELINE, "Bravo");
    let batch = compute_block_lineage(7, &previous, BASELINE, &current, MODIFIED, 50).unwrap();

    let lineage = batch
        .lineages()
        .iter()
        .find(|lineage| lineage.from_ref() == bravo_ref)
        .unwrap();
    assert_eq!(lineage.status(), BlockLineageStatus::Modified);
    assert!(matches!(
        lineage.basis(),
        LineageBasis::ModifiedSimilarity { .. }
    ));
    assert!(!batch.work_plan().items().iter().any(|item| {
        matches!(item, IncrementalWorkItem::AdvanceCurrentProjection { from_ref, .. }
            if *from_ref == bravo_ref)
    }));
    assert!(batch.work_plan().items().iter().any(|item| {
        matches!(item, IncrementalWorkItem::ReviewMemory { from_ref, reason }
            if *from_ref == bravo_ref && *reason == BlockLineageStatus::Modified)
    }));
}

#[test]
fn deletion_keeps_the_historical_reference_and_requests_review() {
    let previous = extraction(BASELINE, 1, 1, 1);
    let current = extraction(DELETED, 2, 2, 100);
    let bravo_ref = ref_for_quote(&previous, BASELINE, "Bravo");
    let batch = compute_block_lineage(7, &previous, BASELINE, &current, DELETED, 50).unwrap();

    let lineage = batch
        .lineages()
        .iter()
        .find(|lineage| lineage.from_ref() == bravo_ref)
        .unwrap();
    assert_eq!(lineage.status(), BlockLineageStatus::Removed);
    assert_eq!(lineage.to_ref(), None);
}

#[test]
fn duplicate_candidates_fail_closed_as_ambiguous() {
    let previous = extraction(AMBIGUOUS_FROM, 1, 1, 1);
    let current = extraction(AMBIGUOUS_TO, 2, 2, 100);
    let batch =
        compute_block_lineage(7, &previous, AMBIGUOUS_FROM, &current, AMBIGUOUS_TO, 50).unwrap();

    let repeated = previous
        .blocks()
        .iter()
        .filter(|block| {
            block
                .anchor()
                .quote(AMBIGUOUS_FROM)
                .unwrap()
                .contains("Repeated")
        })
        .map(|block| {
            batch
                .lineages()
                .iter()
                .find(|lineage| lineage.from_ref() == block.reference())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert!(repeated.iter().all(|lineage| {
        lineage.status() == BlockLineageStatus::Ambiguous
            && lineage.to_ref().is_none()
            && matches!(
                lineage.basis(),
                LineageBasis::AmbiguousCandidates { candidates } if candidates.len() == 2
            )
    }));
}

#[test]
fn stable_native_locator_marks_changed_text_modified_without_projection() {
    let previous = extraction(NATIVE_OLD, 1, 1, 1);
    let current = extraction(NATIVE_NEW, 2, 2, 100);
    let old_ref = ref_for_quote(&previous, NATIVE_OLD, "Original stable text");
    let batch = compute_block_lineage(7, &previous, NATIVE_OLD, &current, NATIVE_NEW, 50).unwrap();

    let lineage = batch
        .lineages()
        .iter()
        .find(|lineage| lineage.from_ref() == old_ref)
        .unwrap();
    assert_eq!(lineage.status(), BlockLineageStatus::Modified);
    assert_eq!(lineage.basis(), &LineageBasis::UniqueNativeLocator);
    assert!(!batch.work_plan().items().iter().any(|item| {
        matches!(item, IncrementalWorkItem::AdvanceCurrentProjection { from_ref, .. }
            if *from_ref == old_ref)
    }));
}

fn extraction(
    source: &str,
    evidence_id: u64,
    revision_id: u64,
    first_block_id: u64,
) -> MaterializedExtraction {
    let parsed = parse_markdown(source, ParseLimits::default()).unwrap();
    let validated = validate_accepted_markdown(evidence_id, source, &parsed, 10).unwrap();
    let revision = ExtractionRevision::new(
        ExtractionRevisionId::new(revision_id).unwrap(),
        evidence_id,
        parsed.contract_version,
        *validated.canonical_digest(),
        10,
    )
    .unwrap();
    let mut assigned = HashMap::new();
    let blocks = validated
        .blocks()
        .iter()
        .enumerate()
        .map(|(offset, draft)| {
            let id = EvidenceBlockId::new(first_block_id + u64::try_from(offset).unwrap()).unwrap();
            let parent_id = draft
                .parent_local_id()
                .map(|local_id| *assigned.get(&local_id).unwrap());
            let block = EvidenceBlock::new(
                id,
                evidence_id,
                revision.id(),
                parent_id,
                draft.ordinal(),
                draft.kind(),
                draft.anchor().clone(),
                draft.metadata().clone(),
            )
            .unwrap();
            assigned.insert(draft.local_id(), id);
            block
        })
        .collect();
    MaterializedExtraction::new(revision, blocks, false).unwrap()
}

fn ref_for_quote(
    extraction: &MaterializedExtraction,
    source: &str,
    needle: &str,
) -> eam_ingestion::EvidenceBlockRef {
    extraction
        .blocks()
        .iter()
        .find(|block| block.anchor().quote(source).unwrap().contains(needle))
        .unwrap()
        .reference()
}

fn quote_for_ref<'a>(
    extraction: &MaterializedExtraction,
    source: &'a str,
    reference: eam_ingestion::EvidenceBlockRef,
) -> &'a str {
    extraction
        .blocks()
        .iter()
        .find(|block| block.reference() == reference)
        .unwrap()
        .anchor()
        .quote(source)
        .unwrap()
}

fn status_for_quote(
    batch: &eam_ingestion::LineageBatch,
    previous: &MaterializedExtraction,
    source: &str,
    needle: &str,
) -> BlockLineageStatus {
    let reference = ref_for_quote(previous, source, needle);
    batch
        .lineages()
        .iter()
        .find(|lineage| lineage.from_ref() == reference)
        .unwrap()
        .status()
}
