use std::{collections::BTreeSet, error::Error, fmt};

use eam_core::{ClaimId, Timestamp};

use crate::IdentityStateVersion;

const INITIAL_CONSTITUTION_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfBundleListField {
    CounterpartExperienceRefs,
    PendingIntentions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfBundleValidationError {
    InvalidConstitutionVersion,
    InvalidIdentityStateVersion,
    EmptyRelationshipState,
    EmptyListItem {
        field: SelfBundleListField,
        index: usize,
    },
    DuplicateListItem {
        field: SelfBundleListField,
        value: String,
    },
    DuplicateBeliefRef(ClaimId),
}

impl fmt::Display for SelfBundleValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for SelfBundleValidationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfBundleState {
    constitution_version: u64,
    identity_state_version: u64,
    counterpart_experience_refs: Vec<String>,
    belief_refs: Vec<ClaimId>,
    relationship_state: String,
    pending_intentions: Vec<String>,
}

impl SelfBundleState {
    /// Builds one complete portable Self Bundle state.
    ///
    /// # Errors
    ///
    /// Rejects zero version identifiers, empty relationship state, empty or
    /// duplicate opaque references, and duplicate belief references.
    pub fn new(
        constitution_version: u64,
        identity_state_version: u64,
        counterpart_experience_refs: Vec<String>,
        belief_refs: Vec<ClaimId>,
        relationship_state: impl Into<String>,
        pending_intentions: Vec<String>,
    ) -> Result<Self, SelfBundleValidationError> {
        if constitution_version == 0 {
            return Err(SelfBundleValidationError::InvalidConstitutionVersion);
        }
        if identity_state_version == 0 {
            return Err(SelfBundleValidationError::InvalidIdentityStateVersion);
        }

        let relationship_state = relationship_state.into();
        if relationship_state.trim().is_empty() {
            return Err(SelfBundleValidationError::EmptyRelationshipState);
        }

        validate_string_list(
            &counterpart_experience_refs,
            SelfBundleListField::CounterpartExperienceRefs,
        )?;
        validate_string_list(&pending_intentions, SelfBundleListField::PendingIntentions)?;

        let mut unique_beliefs = BTreeSet::new();
        for belief_ref in &belief_refs {
            if !unique_beliefs.insert(*belief_ref) {
                return Err(SelfBundleValidationError::DuplicateBeliefRef(*belief_ref));
            }
        }

        Ok(Self {
            constitution_version,
            identity_state_version,
            counterpart_experience_refs,
            belief_refs,
            relationship_state,
            pending_intentions,
        })
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.constitution_version
    }

    #[must_use]
    pub const fn identity_state_version(&self) -> u64 {
        self.identity_state_version
    }

    #[must_use]
    pub fn counterpart_experience_refs(&self) -> &[String] {
        &self.counterpart_experience_refs
    }

    #[must_use]
    pub fn belief_refs(&self) -> &[ClaimId] {
        &self.belief_refs
    }

    #[must_use]
    pub fn relationship_state(&self) -> &str {
        &self.relationship_state
    }

    #[must_use]
    pub fn pending_intentions(&self) -> &[String] {
        &self.pending_intentions
    }
}

fn validate_string_list(
    values: &[String],
    field: SelfBundleListField,
) -> Result<(), SelfBundleValidationError> {
    let mut unique = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            return Err(SelfBundleValidationError::EmptyListItem { field, index });
        }
        if !unique.insert(value.as_str()) {
            return Err(SelfBundleValidationError::DuplicateListItem {
                field,
                value: value.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceState {
    Sleeping,
    LoadSelf,
    Observe,
    Think,
    Respond,
    WriteAgentMemory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeTrigger {
    ConversationStarted,
    EvidenceChanged,
    ScheduledReflection,
    ImportantChange,
}

impl WakeTrigger {
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::ConversationStarted => 0,
            Self::EvidenceChanged => 1,
            Self::ScheduledReflection => 2,
            Self::ImportantChange => 3,
        }
    }

    #[must_use]
    pub const fn from_code(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::ConversationStarted),
            1 => Some(Self::EvidenceChanged),
            2 => Some(Self::ScheduledReflection),
            3 => Some(Self::ImportantChange),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeExit {
    Completed,
    InterruptedAt(PresenceState),
}

impl WakeExit {
    #[must_use]
    pub const fn code(self) -> Option<i64> {
        match self {
            Self::Completed => Some(0),
            Self::InterruptedAt(PresenceState::Observe) => Some(1),
            Self::InterruptedAt(PresenceState::Think) => Some(2),
            Self::InterruptedAt(PresenceState::Respond) => Some(3),
            Self::InterruptedAt(
                PresenceState::Sleeping | PresenceState::LoadSelf | PresenceState::WriteAgentMemory,
            ) => None,
        }
    }

    #[must_use]
    pub const fn from_code(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Completed),
            1 => Some(Self::InterruptedAt(PresenceState::Observe)),
            2 => Some(Self::InterruptedAt(PresenceState::Think)),
            3 => Some(Self::InterruptedAt(PresenceState::Respond)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeCommit {
    trigger: WakeTrigger,
    exit: WakeExit,
}

impl WakeCommit {
    #[must_use]
    pub const fn new(trigger: WakeTrigger, exit: WakeExit) -> Self {
        Self { trigger, exit }
    }

    #[must_use]
    pub const fn trigger(self) -> WakeTrigger {
        self.trigger
    }

    #[must_use]
    pub const fn exit(self) -> WakeExit {
        self.exit
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfBundleVersion {
    version: u64,
    predecessor_version: Option<u64>,
    state: SelfBundleState,
    wake_commit: Option<WakeCommit>,
    committed_at: Timestamp,
}

impl SelfBundleVersion {
    pub(crate) fn initial(identity: &IdentityStateVersion, committed_at: Timestamp) -> Self {
        let state = SelfBundleState {
            constitution_version: INITIAL_CONSTITUTION_VERSION,
            identity_state_version: identity.version(),
            counterpart_experience_refs: Vec::new(),
            belief_refs: Vec::new(),
            relationship_state: identity.profile().relationship_posture().to_owned(),
            pending_intentions: Vec::new(),
        };
        Self::restore(1, None, state, None, committed_at)
    }

    /// Restores an immutable Self Bundle version from a trusted adapter.
    #[must_use]
    pub const fn restore(
        version: u64,
        predecessor_version: Option<u64>,
        state: SelfBundleState,
        wake_commit: Option<WakeCommit>,
        committed_at: Timestamp,
    ) -> Self {
        Self {
            version,
            predecessor_version,
            state,
            wake_commit,
            committed_at,
        }
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub const fn predecessor_version(&self) -> Option<u64> {
        self.predecessor_version
    }

    #[must_use]
    pub const fn state(&self) -> &SelfBundleState {
        &self.state
    }

    #[must_use]
    pub const fn wake_commit(&self) -> Option<WakeCommit> {
        self.wake_commit
    }

    #[must_use]
    pub const fn committed_at(&self) -> Timestamp {
        self.committed_at
    }
}
