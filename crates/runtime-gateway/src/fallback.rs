use eam_core::{
    ConversationEvidence, CounterpartRuntime, PersonTurnClassification, RuntimeError,
    RuntimeErrorKind, RuntimeRequest, RuntimeResponse,
};

pub struct FallbackRuntime<P, F> {
    primary: P,
    fallback: F,
}

impl<P, F> FallbackRuntime<P, F> {
    #[must_use]
    pub const fn new(primary: P, fallback: F) -> Self {
        Self { primary, fallback }
    }

    #[must_use]
    pub const fn primary(&self) -> &P {
        &self.primary
    }

    #[must_use]
    pub const fn fallback(&self) -> &F {
        &self.fallback
    }

    #[must_use]
    pub fn into_parts(self) -> (P, F) {
        (self.primary, self.fallback)
    }
}

impl<P, F> CounterpartRuntime for FallbackRuntime<P, F>
where
    P: CounterpartRuntime,
    F: CounterpartRuntime,
{
    fn classify_person_turn(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonTurnClassification, RuntimeError> {
        match self.primary.classify_person_turn(evidence) {
            Err(error) if retryable(error.kind()) => self.fallback.classify_person_turn(evidence),
            result => result,
        }
    }

    fn respond(&mut self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        match self.primary.respond(request.clone()) {
            Err(error) if retryable(error.kind()) => self.fallback.respond(request),
            result => result,
        }
    }
}

const fn retryable(kind: RuntimeErrorKind) -> bool {
    matches!(
        kind,
        RuntimeErrorKind::Timeout | RuntimeErrorKind::Unavailable
    )
}
