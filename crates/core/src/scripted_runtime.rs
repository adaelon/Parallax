use std::collections::VecDeque;

use crate::{
    Clock, ConversationEvidence, CounterpartRuntime, PersonTurnClassification, RuntimeError,
    RuntimeRequest, RuntimeResponse, Timestamp,
};

#[derive(Debug, Default)]
pub struct ScriptedRuntime {
    classifications: VecDeque<PersonTurnClassification>,
    responses: VecDeque<RuntimeResponse>,
    seen_classification_inputs: Vec<ConversationEvidence>,
    seen_requests: Vec<RuntimeRequest>,
}

impl ScriptedRuntime {
    #[must_use]
    pub fn new(
        classifications: impl IntoIterator<Item = PersonTurnClassification>,
        responses: impl IntoIterator<Item = RuntimeResponse>,
    ) -> Self {
        Self {
            classifications: classifications.into_iter().collect(),
            responses: responses.into_iter().collect(),
            seen_classification_inputs: Vec::new(),
            seen_requests: Vec::new(),
        }
    }

    #[must_use]
    pub fn seen_classification_inputs(&self) -> &[ConversationEvidence] {
        &self.seen_classification_inputs
    }

    #[must_use]
    pub fn seen_requests(&self) -> &[RuntimeRequest] {
        &self.seen_requests
    }
}

impl CounterpartRuntime for ScriptedRuntime {
    fn classify_person_turn(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonTurnClassification, RuntimeError> {
        self.seen_classification_inputs.push(evidence.clone());
        self.classifications
            .pop_front()
            .ok_or_else(|| RuntimeError::new("no scripted person-turn classification remains"))
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
