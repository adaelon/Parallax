use std::collections::BTreeMap;

use crate::{
    Claim, ClaimCorrectionReceipt, ClaimCorrectionRepository, ClaimId, ClaimOwner, ClaimStatus,
    ConversationEvidence, EvidenceId, ForgetReceipt, ForgetRepository, ForgetTarget,
    MemoryRepository, RepositoryError, SharedAgreementCandidate, SharedAgreementCandidateId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementResolution,
    SharedExperience, SharedExperienceKind, SharedExperienceRepository, Speaker, Timestamp,
};

#[derive(Debug)]
pub struct InMemoryRepository {
    next_evidence_id: u64,
    next_claim_id: u64,
    evidence: BTreeMap<EvidenceId, ConversationEvidence>,
    claims: BTreeMap<ClaimId, Claim>,
    next_shared_agreement_candidate_id: u64,
    shared_agreement_candidates: BTreeMap<SharedAgreementCandidateId, SharedAgreementCandidate>,
    shared_experiences: BTreeMap<ClaimId, SharedExperience>,
    deletion_intents: BTreeMap<ForgetTarget, ForgetReceipt>,
    next_deletion_intent_id: u64,
}

impl Default for InMemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRepository {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_evidence_id: 1,
            next_claim_id: 1,
            evidence: BTreeMap::new(),
            claims: BTreeMap::new(),
            next_shared_agreement_candidate_id: 1,
            shared_agreement_candidates: BTreeMap::new(),
            shared_experiences: BTreeMap::new(),
            deletion_intents: BTreeMap::new(),
            next_deletion_intent_id: 1,
        }
    }
}

impl SharedExperienceRepository for InMemoryRepository {
    fn next_shared_agreement_candidate_id(&mut self) -> SharedAgreementCandidateId {
        let id = SharedAgreementCandidateId::from_raw(self.next_shared_agreement_candidate_id);
        self.next_shared_agreement_candidate_id = self
            .next_shared_agreement_candidate_id
            .checked_add(1)
            .expect("shared agreement candidate identifier space exhausted");
        id
    }

    fn stage_shared_agreement_candidate(
        &mut self,
        candidate: SharedAgreementCandidate,
    ) -> Result<(), RepositoryError> {
        validate_candidate_storage(&self.evidence, &candidate)?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson
            || candidate.counterpart_assented_at().is_none()
            || candidate.decided_at().is_some()
            || candidate.claim_id().is_some()
        {
            return Err(RepositoryError::new(
                "new shared agreement candidate must await person confirmation",
            ));
        }
        if self
            .shared_agreement_candidates
            .insert(candidate.id(), candidate)
            .is_some()
        {
            return Err(RepositoryError::new(
                "duplicate shared agreement candidate id",
            ));
        }
        Ok(())
    }

    fn commit_shared_agreement_revision(
        &mut self,
        previous_id: SharedAgreementCandidateId,
        person_evidence: ConversationEvidence,
        revised: SharedAgreementCandidate,
        revised_at: Timestamp,
    ) -> Result<(), RepositoryError> {
        let previous = self
            .shared_agreement_candidates
            .get(&previous_id)
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if previous.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting person confirmation",
            ));
        }
        if revised.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || revised.predecessor_candidate_id() != Some(previous_id)
            || revised.version() != previous.version().saturating_add(1)
            || revised.counterpart_assented_at().is_some()
            || revised.decided_at().is_some()
            || revised.claim_id().is_some()
            || person_evidence.speaker() != Speaker::Person
            || self.evidence.contains_key(&person_evidence.id())
            || self.shared_agreement_candidates.contains_key(&revised.id())
        {
            return Err(RepositoryError::new("invalid shared agreement revision"));
        }
        let mut evidence = self.evidence.clone();
        evidence.insert(person_evidence.id(), person_evidence.clone());
        validate_candidate_storage(&evidence, &revised)?;

        self.evidence.insert(person_evidence.id(), person_evidence);
        self.shared_agreement_candidates
            .get_mut(&previous_id)
            .expect("validated candidate remains present")
            .resolve(SharedAgreementCandidateStatus::Deferred, revised_at, None);
        self.shared_agreement_candidates
            .insert(revised.id(), revised);
        Ok(())
    }

    fn commit_counterpart_agreement_assent(
        &mut self,
        id: SharedAgreementCandidateId,
        version: u64,
        citation: crate::EvidenceCitation,
        assented_at: Timestamp,
    ) -> Result<SharedAgreementCandidate, RepositoryError> {
        let source = self
            .evidence
            .get(&citation.evidence_id())
            .ok_or_else(|| RepositoryError::new("counterpart assent evidence does not exist"))?;
        if source.speaker() != Speaker::Counterpart
            || citation.quote().is_empty()
            || !source.verbatim().contains(citation.quote())
        {
            return Err(RepositoryError::new(
                "counterpart assent is not an exact counterpart quote",
            ));
        }
        let candidate = self
            .shared_agreement_candidates
            .get_mut(&id)
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || candidate.version() != version
        {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting this counterpart assent",
            ));
        }
        candidate.accept_counterpart_assent(citation, assented_at);
        Ok(candidate.clone())
    }

    fn shared_agreement_candidate(
        &self,
        id: SharedAgreementCandidateId,
    ) -> Result<Option<SharedAgreementCandidate>, RepositoryError> {
        Ok(self.shared_agreement_candidates.get(&id).cloned())
    }

    fn commit_shared_agreement_decision(
        &mut self,
        id: SharedAgreementCandidateId,
        decision: SharedAgreementDecision,
        confirmed: Option<SharedExperience>,
        decided_at: Timestamp,
    ) -> Result<SharedAgreementResolution, RepositoryError> {
        let candidate = self
            .shared_agreement_candidates
            .get(&id)
            .cloned()
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting person confirmation",
            ));
        }

        let (status, claim_id) = match decision {
            SharedAgreementDecision::Confirm => {
                let experience = confirmed.ok_or_else(|| {
                    RepositoryError::new("confirmed agreement requires a shared claim")
                })?;
                if experience.kind() != SharedExperienceKind::Agreement
                    || experience.claim().statement() != candidate.statement()
                    || experience.claim().support() != candidate.support()
                    || candidate.effective_from().is_none()
                    || experience.claim().applicable_time() != agreement_applicable_time(&candidate)
                {
                    return Err(RepositoryError::new(
                        "confirmed agreement does not match its immutable candidate",
                    ));
                }
                validate_shared_experience_storage(&self.evidence, &experience)?;
                let claim_id = experience.claim().id();
                if self.claims.contains_key(&claim_id)
                    || self.shared_experiences.contains_key(&claim_id)
                {
                    return Err(RepositoryError::new("duplicate shared claim id"));
                }
                self.claims.insert(claim_id, experience.claim().clone());
                self.shared_experiences.insert(claim_id, experience);
                (SharedAgreementCandidateStatus::Confirmed, Some(claim_id))
            }
            SharedAgreementDecision::Defer => {
                if confirmed.is_some() {
                    return Err(RepositoryError::new(
                        "deferred agreement cannot append a shared claim",
                    ));
                }
                (SharedAgreementCandidateStatus::Deferred, None)
            }
        };

        self.shared_agreement_candidates
            .get_mut(&id)
            .expect("validated candidate remains present")
            .resolve(status, decided_at, claim_id);
        Ok(SharedAgreementResolution::new(id, status, claim_id))
    }

    fn commit_shared_experience(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        if experience.kind() == SharedExperienceKind::Agreement {
            return Err(RepositoryError::new(
                "shared agreements require a person-confirmed candidate",
            ));
        }
        validate_shared_experience_storage(&self.evidence, &experience)?;
        let claim_id = experience.claim().id();
        if self.claims.contains_key(&claim_id) || self.shared_experiences.contains_key(&claim_id) {
            return Err(RepositoryError::new("duplicate shared claim id"));
        }
        self.claims.insert(claim_id, experience.claim().clone());
        self.shared_experiences.insert(claim_id, experience);
        Ok(())
    }

    fn all_shared_agreement_candidates(
        &self,
    ) -> Result<Vec<SharedAgreementCandidate>, RepositoryError> {
        Ok(self.shared_agreement_candidates.values().cloned().collect())
    }

    fn all_shared_experiences(&self) -> Result<Vec<SharedExperience>, RepositoryError> {
        Ok(self.shared_experiences.values().cloned().collect())
    }

    fn dismiss_shared_experience_ceremony(
        &mut self,
        claim_id: ClaimId,
    ) -> Result<bool, RepositoryError> {
        let Some(experience) = self.shared_experiences.get_mut(&claim_id) else {
            return Ok(false);
        };
        experience.dismiss_ceremony();
        Ok(true)
    }
}

fn validate_candidate_storage(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    candidate: &SharedAgreementCandidate,
) -> Result<(), RepositoryError> {
    if candidate.version() == 0
        || candidate.statement().trim().is_empty()
        || candidate
            .scope()
            .is_none_or(|scope| scope.trim().is_empty())
        || candidate.effective_from().is_none()
        || candidate.effective_until().is_some_and(|until| {
            until.as_millis()
                < candidate
                    .effective_from()
                    .expect("checked above")
                    .as_millis()
        })
        || candidate
            .end_condition()
            .is_some_and(|condition| condition.trim().is_empty())
    {
        return Err(RepositoryError::new("invalid shared agreement candidate"));
    }
    match candidate.status() {
        SharedAgreementCandidateStatus::AwaitingCounterpart => {
            validate_candidate_support(evidence, candidate.support(), true, false)
        }
        SharedAgreementCandidateStatus::AwaitingPerson
        | SharedAgreementCandidateStatus::Deferred
        | SharedAgreementCandidateStatus::Confirmed => {
            validate_candidate_support(evidence, candidate.support(), true, true)
        }
    }
}

fn agreement_applicable_time(candidate: &SharedAgreementCandidate) -> crate::ApplicableTime {
    let start = candidate
        .effective_from()
        .expect("signable candidates have an effective time");
    candidate
        .effective_until()
        .map_or(crate::ApplicableTime::Since(start), |end| {
            crate::ApplicableTime::Between { start, end }
        })
}

fn validate_candidate_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
    require_person: bool,
    require_counterpart: bool,
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(evidence, support)?;
    if (require_person && !has_person) || (require_counterpart && !has_counterpart) {
        return Err(RepositoryError::new(
            "candidate signature evidence does not match its signing state",
        ));
    }
    Ok(())
}

fn validate_shared_experience_storage(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    experience: &SharedExperience,
) -> Result<(), RepositoryError> {
    let claim = experience.claim();
    if claim.owner() != ClaimOwner::Shared
        || claim.status() != ClaimStatus::Current
        || claim.statement().trim().is_empty()
        || claim.support().len() < 2
    {
        return Err(RepositoryError::new("invalid shared experience claim"));
    }
    validate_shared_support(evidence, claim.support())
}

fn validate_shared_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(evidence, support)?;
    if !has_person || !has_counterpart {
        return Err(RepositoryError::new(
            "shared history requires evidence from both participants",
        ));
    }
    Ok(())
}

fn validate_exact_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
) -> Result<(bool, bool), RepositoryError> {
    let mut has_person = false;
    let mut has_counterpart = false;
    for citation in support {
        let source = evidence
            .get(&citation.evidence_id())
            .ok_or_else(|| RepositoryError::new("shared support evidence does not exist"))?;
        if citation.quote().is_empty() || !source.verbatim().contains(citation.quote()) {
            return Err(RepositoryError::new(
                "shared support is not an exact evidence quote",
            ));
        }
        match source.speaker() {
            Speaker::Person => has_person = true,
            Speaker::Counterpart => has_counterpart = true,
        }
    }
    Ok((has_person, has_counterpart))
}

impl ForgetRepository for InMemoryRepository {
    fn commit_forget(
        &mut self,
        target: ForgetTarget,
        _requested_at: Timestamp,
    ) -> Result<Option<ForgetReceipt>, RepositoryError> {
        if let Some(receipt) = self.deletion_intents.get(&target) {
            return Ok(Some(*receipt));
        }
        let ForgetTarget::ConversationEvidence(evidence_id) = target else {
            return Ok(None);
        };
        if !self.evidence.contains_key(&evidence_id) {
            return Ok(None);
        }

        let mut affected_claims = self
            .claims
            .values()
            .filter(|claim| {
                claim
                    .support()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
            })
            .map(Claim::id)
            .collect::<Vec<_>>();
        let mut affected_candidates = self
            .shared_agreement_candidates
            .values()
            .filter(|candidate| {
                candidate
                    .support()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
                    || candidate
                        .claim_id()
                        .is_some_and(|claim_id| affected_claims.contains(&claim_id))
            })
            .map(SharedAgreementCandidate::id)
            .collect::<Vec<_>>();
        loop {
            let previous_len = affected_candidates.len();
            for candidate in self.shared_agreement_candidates.values() {
                if candidate
                    .predecessor_candidate_id()
                    .is_some_and(|id| affected_candidates.contains(&id))
                    && !affected_candidates.contains(&candidate.id())
                {
                    affected_candidates.push(candidate.id());
                }
            }
            if affected_candidates.len() == previous_len {
                break;
            }
        }
        affected_claims.extend(
            self.shared_agreement_candidates
                .values()
                .filter(|candidate| affected_candidates.contains(&candidate.id()))
                .filter_map(SharedAgreementCandidate::claim_id),
        );
        loop {
            let previous_len = affected_claims.len();
            for claim in self.claims.values() {
                let linked = claim
                    .supersedes()
                    .is_some_and(|id| affected_claims.contains(&id))
                    || claim
                        .superseded_by()
                        .is_some_and(|id| affected_claims.contains(&id));
                if linked && !affected_claims.contains(&claim.id()) {
                    affected_claims.push(claim.id());
                }
            }
            if affected_claims.len() == previous_len {
                break;
            }
        }
        for claim_id in &affected_claims {
            self.claims.remove(claim_id);
        }
        for candidate_id in &affected_candidates {
            self.shared_agreement_candidates.remove(candidate_id);
        }
        let affected_experiences = self
            .shared_experiences
            .keys()
            .filter(|claim_id| affected_claims.contains(claim_id))
            .copied()
            .collect::<Vec<_>>();
        for claim_id in &affected_experiences {
            self.shared_experiences.remove(claim_id);
        }
        self.evidence.remove(&evidence_id);

        let receipt = ForgetReceipt::new(
            self.next_deletion_intent_id,
            target,
            1 + affected_claims.len(),
            affected_candidates.len() + affected_experiences.len(),
            0,
        );
        self.next_deletion_intent_id += 1;
        self.deletion_intents.insert(target, receipt);
        Ok(Some(receipt))
    }
}

impl MemoryRepository for InMemoryRepository {
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
        if self.evidence.insert(evidence.id(), evidence).is_some() {
            return Err(RepositoryError::new("duplicate evidence id"));
        }
        Ok(())
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        if self.claims.insert(claim.id(), claim).is_some() {
            return Err(RepositoryError::new("duplicate claim id"));
        }
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.get(&id).cloned())
    }

    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.values().cloned().collect())
    }

    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError> {
        Ok(self.claims.values().cloned().collect())
    }
}

impl ClaimCorrectionRepository for InMemoryRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        Ok(self.claims.get(&id).cloned())
    }

    fn commit_person_fact_correction(
        &mut self,
        evidence: ConversationEvidence,
        replacement: Claim,
    ) -> Result<ClaimCorrectionReceipt, RepositoryError> {
        let superseded_id = replacement
            .supersedes()
            .ok_or_else(|| RepositoryError::new("correction claim has no predecessor"))?;
        let previous = self
            .claims
            .get(&superseded_id)
            .ok_or_else(|| RepositoryError::new("claim does not exist"))?;
        if previous.owner() != ClaimOwner::Person || replacement.owner() != ClaimOwner::Person {
            return Err(RepositoryError::new("only person claims can be corrected"));
        }
        if previous.status() != ClaimStatus::Current {
            return Err(RepositoryError::new("claim is not current"));
        }
        if self.evidence.contains_key(&evidence.id()) || self.claims.contains_key(&replacement.id())
        {
            return Err(RepositoryError::new("duplicate correction identifier"));
        }

        self.evidence.insert(evidence.id(), evidence.clone());
        self.claims
            .get_mut(&superseded_id)
            .expect("validated predecessor remains present")
            .mark_superseded_by(replacement.id());
        self.claims.insert(replacement.id(), replacement.clone());
        Ok(ClaimCorrectionReceipt::new(
            evidence.id(),
            superseded_id,
            replacement.id(),
            0,
            0,
            0,
            0,
        ))
    }
}
