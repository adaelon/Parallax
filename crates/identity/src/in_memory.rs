use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    MemoryRepository, RepositoryError, SessionId, Speaker, Timestamp,
};

use crate::{
    IdentityRepository, IdentityStateVersion, InitialSelfIntroduction, IntroductionAnswer,
    IntroductionItem, SelfBundleRepository, SelfBundleVersion, SelfIntroductionCategory,
};

pub struct InMemoryIdentityRepository {
    next_evidence_id: u64,
    next_claim_id: u64,
    evidence: Vec<ConversationEvidence>,
    claims: Vec<Claim>,
    introduction: Option<InitialSelfIntroduction>,
    identities: Vec<IdentityStateVersion>,
    self_bundles: Vec<SelfBundleVersion>,
}

impl InMemoryIdentityRepository {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_evidence_id: 1,
            next_claim_id: 1,
            evidence: Vec::new(),
            claims: Vec::new(),
            introduction: None,
            identities: Vec::new(),
            self_bundles: Vec::new(),
        }
    }
}

impl Default for InMemoryIdentityRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityRepository for InMemoryIdentityRepository {
    fn record_initial_self_introduction(
        &mut self,
        session_id: &SessionId,
        answers: &[IntroductionAnswer],
        recorded_at: Timestamp,
    ) -> Result<InitialSelfIntroduction, RepositoryError> {
        if self.introduction.is_some() {
            return Err(RepositoryError::new(
                "initial self introduction already exists",
            ));
        }

        let mut items = Vec::with_capacity(SelfIntroductionCategory::ALL.len());
        for category in SelfIntroductionCategory::ALL {
            let answer = answers
                .iter()
                .find(|answer| answer.category() == category)
                .ok_or_else(|| RepositoryError::new("validated introduction category missing"))?;
            let evidence_id = self.next_evidence_id();
            let claim_id = self.next_claim_id();
            let evidence = ConversationEvidence::restore(
                evidence_id,
                session_id.clone(),
                Speaker::Person,
                answer.statement().to_owned(),
                recorded_at,
            );
            let claim = Claim::restore(
                claim_id,
                ClaimOwner::Person,
                answer.statement().to_owned(),
                vec![EvidenceCitation::new(evidence_id, answer.statement())],
                None,
                ApplicableTime::At(recorded_at),
                recorded_at,
            );
            self.append_evidence(evidence)?;
            self.append_claim(claim)?;
            items.push(IntroductionItem::restore(
                category,
                evidence_id,
                claim_id,
                answer.statement(),
                recorded_at,
            ));
        }

        let introduction = InitialSelfIntroduction::restore(session_id.clone(), items);
        self.introduction = Some(introduction.clone());
        Ok(introduction)
    }

    fn initial_self_introduction(
        &self,
    ) -> Result<Option<InitialSelfIntroduction>, RepositoryError> {
        Ok(self.introduction.clone())
    }

    fn append_identity_state(
        &mut self,
        identity: IdentityStateVersion,
    ) -> Result<(), RepositoryError> {
        match self.identities.last() {
            None if identity.version() == 1 && identity.predecessor_version().is_none() => {}
            Some(current)
                if identity.version() == current.version().saturating_add(1)
                    && identity.predecessor_version() == Some(current.version()) => {}
            _ => {
                return Err(RepositoryError::new(
                    "identity version does not continue the current immutable chain",
                ));
            }
        }
        self.identities.push(identity);
        Ok(())
    }

    fn current_identity_state(&self) -> Result<Option<IdentityStateVersion>, RepositoryError> {
        Ok(self.identities.last().cloned())
    }

    fn all_identity_states(&self) -> Result<Vec<IdentityStateVersion>, RepositoryError> {
        Ok(self.identities.clone())
    }
}

impl SelfBundleRepository for InMemoryIdentityRepository {
    fn append_self_bundle(&mut self, bundle: SelfBundleVersion) -> Result<(), RepositoryError> {
        match self.self_bundles.last() {
            None => {
                if bundle.version() != 1
                    || bundle.predecessor_version().is_some()
                    || bundle.wake_commit().is_some()
                {
                    return Err(RepositoryError::new(
                        "initial Self Bundle must be version 1 without predecessor or wake commit",
                    ));
                }
            }
            Some(current) => {
                let expected_version = current
                    .version()
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("Self Bundle version space exhausted"))?;
                if bundle.version() != expected_version
                    || bundle.predecessor_version() != Some(current.version())
                    || bundle.wake_commit().is_none()
                {
                    return Err(RepositoryError::new(
                        "Self Bundle version does not continue the current immutable chain",
                    ));
                }
            }
        }
        self.self_bundles.push(bundle);
        Ok(())
    }

    fn current_self_bundle(&self) -> Result<Option<SelfBundleVersion>, RepositoryError> {
        Ok(self.self_bundles.last().cloned())
    }
}

impl MemoryRepository for InMemoryIdentityRepository {
    fn next_evidence_id(&mut self) -> EvidenceId {
        let id = EvidenceId::from_raw(self.next_evidence_id);
        self.next_evidence_id += 1;
        id
    }

    fn next_claim_id(&mut self) -> ClaimId {
        let id = ClaimId::from_raw(self.next_claim_id);
        self.next_claim_id += 1;
        id
    }

    fn append_evidence(&mut self, evidence: ConversationEvidence) -> Result<(), RepositoryError> {
        self.evidence.push(evidence);
        Ok(())
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        self.claims.push(claim);
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        Ok(self
            .evidence
            .iter()
            .find(|evidence| evidence.id() == id)
            .cloned())
    }

    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.clone())
    }

    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError> {
        Ok(self.claims.clone())
    }
}
