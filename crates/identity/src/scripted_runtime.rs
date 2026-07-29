use std::collections::VecDeque;

use eam_core::RuntimeError;

use crate::{IdentityRuntime, InitialIdentityProposal, InitialIdentityRequest};

pub struct ScriptedIdentityRuntime {
    proposals: VecDeque<InitialIdentityProposal>,
    seen_requests: Vec<InitialIdentityRequest>,
}

impl ScriptedIdentityRuntime {
    #[must_use]
    pub fn new(proposals: impl IntoIterator<Item = InitialIdentityProposal>) -> Self {
        Self {
            proposals: proposals.into_iter().collect(),
            seen_requests: Vec::new(),
        }
    }

    #[must_use]
    pub fn seen_requests(&self) -> &[InitialIdentityRequest] {
        &self.seen_requests
    }
}

impl IdentityRuntime for ScriptedIdentityRuntime {
    fn form_initial_identity(
        &mut self,
        request: InitialIdentityRequest,
    ) -> Result<InitialIdentityProposal, RuntimeError> {
        self.seen_requests.push(request);
        self.proposals
            .pop_front()
            .ok_or_else(|| RuntimeError::new("no scripted identity proposal remains"))
    }
}
