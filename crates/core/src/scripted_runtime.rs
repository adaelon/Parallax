use std::collections::VecDeque;

use crate::{
    ApplicableTime, ClaimOwner, Clock, ConversationEvidence, CounterpartRuntime, EvidenceCitation,
    PersonFactProposal, PersonFactProposalBatch, RuntimeError, RuntimeRequest, RuntimeResponse,
    Timestamp,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptedPersonFactResponse {
    NoFacts,
    VerbatimFactAtRecordedTime,
    Exact(PersonFactProposalBatch),
}

#[derive(Debug, Default)]
pub struct ScriptedRuntime {
    person_fact_responses: VecDeque<ScriptedPersonFactResponse>,
    responses: VecDeque<RuntimeResponse>,
    seen_person_fact_inputs: Vec<ConversationEvidence>,
    seen_requests: Vec<RuntimeRequest>,
}

impl ScriptedRuntime {
    #[must_use]
    pub fn new(
        person_fact_responses: impl IntoIterator<Item = ScriptedPersonFactResponse>,
        responses: impl IntoIterator<Item = RuntimeResponse>,
    ) -> Self {
        Self {
            person_fact_responses: person_fact_responses.into_iter().collect(),
            responses: responses.into_iter().collect(),
            seen_person_fact_inputs: Vec::new(),
            seen_requests: Vec::new(),
        }
    }

    #[must_use]
    pub fn seen_person_fact_inputs(&self) -> &[ConversationEvidence] {
        &self.seen_person_fact_inputs
    }

    #[must_use]
    pub fn seen_requests(&self) -> &[RuntimeRequest] {
        &self.seen_requests
    }
}

impl CounterpartRuntime for ScriptedRuntime {
    fn propose_person_facts(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonFactProposalBatch, RuntimeError> {
        self.seen_person_fact_inputs.push(evidence.clone());
        let response = self.person_fact_responses.pop_front().ok_or_else(|| {
            RuntimeError::new("no scripted person-fact proposal response remains")
        })?;
        match response {
            ScriptedPersonFactResponse::NoFacts => Ok(PersonFactProposalBatch::empty()),
            ScriptedPersonFactResponse::VerbatimFactAtRecordedTime => {
                PersonFactProposalBatch::try_new([PersonFactProposal::new(
                    ClaimOwner::Person,
                    evidence.verbatim(),
                    EvidenceCitation::new(evidence.id(), evidence.verbatim()),
                    ApplicableTime::At(evidence.recorded_at()),
                )])
                .map_err(|error| RuntimeError::invalid_response(error.to_string()))
            }
            ScriptedPersonFactResponse::Exact(proposals) => Ok(proposals),
        }
    }

    fn respond(&mut self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        self.seen_requests.push(request);
        self.responses
            .pop_front()
            .ok_or_else(|| RuntimeError::new("no scripted runtime response remains"))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IncrementingClock {
    next_millis: i64,
}

impl IncrementingClock {
    #[must_use]
    pub const fn new(first_millis: i64) -> Self {
        Self {
            next_millis: first_millis,
        }
    }
}

impl Clock for IncrementingClock {
    fn now(&mut self) -> Timestamp {
        let now = Timestamp::from_millis(self.next_millis);
        self.next_millis += 1;
        now
    }
}
