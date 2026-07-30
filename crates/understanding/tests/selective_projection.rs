use std::{collections::HashMap, convert::Infallible};

use eam_ingestion::{EvidenceBlockId, EvidenceBlockRef};
use eam_understanding::{
    ProjectionBuild, ProjectionContent, ProjectionId, ProjectionKind, ProjectionRecipe,
    ProjectionSource, ProjectionStatus, ProjectionTrigger, SourcedStatement, StoredProjection,
    StoredProjectionRecipe, UnderstandingError, UnderstandingRepository, materialize_projection,
    rebuild_projection,
};

#[derive(Clone, Default)]
struct FixtureRepository {
    authority: HashMap<EvidenceBlockRef, ProjectionSource>,
    recipes: HashMap<ProjectionId, ProjectionRecipe>,
    stored: HashMap<ProjectionId, StoredProjection>,
    artifact_present: HashMap<ProjectionId, bool>,
    resolved: Vec<EvidenceBlockRef>,
}

impl UnderstandingRepository for FixtureRepository {
    type Error = Infallible;

    fn resolve_projection_source(
        &self,
        reference: EvidenceBlockRef,
    ) -> Result<Option<ProjectionSource>, Self::Error> {
        Ok(self.authority.get(&reference).cloned())
    }

    fn commit_projection(
        &mut self,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error> {
        let id = ProjectionId::new(u64::try_from(self.stored.len() + 1).unwrap()).unwrap();
        let projection =
            StoredProjection::new(id, 1, ProjectionStatus::Active, *build.material_digest());
        self.recipes.insert(id, build.recipe().clone());
        self.stored.insert(id, projection.clone());
        self.artifact_present.insert(id, true);
        self.resolved
            .extend(build.sources().iter().map(ProjectionSource::reference));
        Ok(projection)
    }

    fn load_projection_recipe(
        &self,
        id: ProjectionId,
    ) -> Result<Option<StoredProjectionRecipe>, Self::Error> {
        Ok(self
            .stored
            .get(&id)
            .cloned()
            .zip(self.recipes.get(&id).cloned())
            .map(|(projection, recipe)| StoredProjectionRecipe::new(projection, recipe)))
    }

    fn replace_projection_artifact(
        &mut self,
        id: ProjectionId,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error> {
        let previous = self.stored.get(&id).unwrap();
        let projection = StoredProjection::new(
            id,
            previous.generation(),
            previous.status(),
            *build.material_digest(),
        );
        self.stored.insert(id, projection.clone());
        self.artifact_present.insert(id, true);
        Ok(projection)
    }
}

#[test]
fn all_four_triggers_materialize_only_the_explicit_finite_scope() {
    let triggers = [
        ProjectionTrigger::PersonDesignated {
            reason: "本人指定 Aurora".to_owned(),
        },
        ProjectionTrigger::RepeatedRecall {
            query: "Aurora".to_owned(),
            recall_count: 2,
        },
        ProjectionTrigger::ImportantChange {
            description: "Aurora milestone changed".to_owned(),
        },
        ProjectionTrigger::CurrentTask {
            task: "review Aurora history".to_owned(),
        },
    ];
    for trigger in triggers {
        let first = reference(1, 11);
        let second = reference(2, 21);
        let unrelated = reference(3, 31);
        let mut repository = fixture_repository(&[first, second, unrelated]);
        let recipe = ProjectionRecipe::new(
            trigger,
            "Aurora",
            ProjectionContent::EventChain(vec![statement(
                "Aurora moved from planning to delivery",
                vec![first, second],
            )]),
            100,
        )
        .unwrap();

        let projection = materialize_projection(&mut repository, recipe).unwrap();

        assert_eq!(projection.status(), ProjectionStatus::Active);
        assert_eq!(repository.resolved, vec![first, second]);
        assert!(!repository.resolved.contains(&unrelated));
    }
}

#[test]
fn all_three_projection_shapes_preserve_explicit_sources() {
    let source = reference(1, 11);
    let contents = [
        ProjectionContent::EventChain(vec![statement("event", vec![source])]),
        ProjectionContent::PersonTopicRelations(vec![statement(
            "Mina relates to Aurora",
            vec![source],
        )]),
        ProjectionContent::PhaseSummary(statement("delivery phase", vec![source])),
    ];
    let expected = [
        ProjectionKind::EventChain,
        ProjectionKind::PersonTopicRelations,
        ProjectionKind::PhaseSummary,
    ];
    for (content, expected_kind) in contents.into_iter().zip(expected) {
        let recipe = ProjectionRecipe::new(
            ProjectionTrigger::CurrentTask {
                task: "inspect".to_owned(),
            },
            "Aurora",
            content,
            100,
        )
        .unwrap();
        assert_eq!(recipe.content().kind(), expected_kind);
        assert_eq!(recipe.sources(), vec![source]);
    }
}

#[test]
fn non_trigger_and_unsourced_content_are_rejected_before_materialization() {
    let source = reference(1, 11);
    let result = ProjectionRecipe::new(
        ProjectionTrigger::RepeatedRecall {
            query: "only once".to_owned(),
            recall_count: 1,
        },
        "Aurora",
        ProjectionContent::PhaseSummary(statement("summary", vec![source])),
        100,
    );
    assert!(matches!(
        result,
        Err(UnderstandingError::TriggerNotEligible)
    ));
    assert!(matches!(
        SourcedStatement::new("summary", Vec::new()),
        Err(UnderstandingError::InvalidSourceScope)
    ));
}

#[test]
fn deleted_artifact_rebuilds_the_same_contract_generation_and_digest() {
    let source = reference(1, 11);
    let mut repository = fixture_repository(&[source]);
    let recipe = ProjectionRecipe::new(
        ProjectionTrigger::PersonDesignated {
            reason: "keep this phase map".to_owned(),
        },
        "Aurora",
        ProjectionContent::PhaseSummary(statement("delivery phase", vec![source])),
        100,
    )
    .unwrap();
    let first = materialize_projection(&mut repository, recipe).unwrap();
    repository.artifact_present.insert(first.id(), false);

    let rebuilt = rebuild_projection(&mut repository, first.id()).unwrap();

    assert_eq!(rebuilt.generation(), first.generation());
    assert_eq!(rebuilt.material_digest(), first.material_digest());
    assert_eq!(repository.artifact_present.get(&first.id()), Some(&true));
}

fn fixture_repository(references: &[EvidenceBlockRef]) -> FixtureRepository {
    let authority = references
        .iter()
        .enumerate()
        .map(|(ordinal, reference)| {
            (
                *reference,
                ProjectionSource::new(
                    *reference,
                    format!("authority block {ordinal}"),
                    u64::try_from(ordinal + 1).unwrap(),
                    format!("source-{ordinal}.md"),
                    i64::try_from(ordinal).unwrap(),
                ),
            )
        })
        .collect();
    FixtureRepository {
        authority,
        ..FixtureRepository::default()
    }
}

fn reference(evidence_id: u64, block_id: u64) -> EvidenceBlockRef {
    EvidenceBlockRef::new(evidence_id, EvidenceBlockId::new(block_id).unwrap()).unwrap()
}

fn statement(text: &str, sources: Vec<EvidenceBlockRef>) -> SourcedStatement {
    SourcedStatement::new(text, sources).unwrap()
}
