use std::fmt;

/// Errors that can occur while creating or applying a state delta.
///
/// Delta application can fail when the delta is incompatible with the target
/// state or when the delta payload cannot be interpreted safely. This error
/// type distinguishes structural incompatibilities from malformed data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The delta was created for a different Ethereum consensus fork than
    /// the state it is being applied to.
    ///
    /// A delta must only be applied to a state from the same fork because
    /// state fields and their SSZ layouts may differ between forks.
    ForkMismatch {
        /// Fork of the state receiving the delta.
        state_fork: crate::ForkName,

        /// Fork for which the delta was created.
        delta_fork: crate::ForkName,
    },

    /// The delta contains a field that is not part of the target fork's
    /// state representation.
    ///
    /// This generally indicates an incorrectly constructed delta or an
    /// attempt to apply a delta using the wrong fork-specific schema.
    InvalidFieldForFork {
        /// Name of the field included in the delta.
        field: &'static str,

        /// Fork against which the field was validated.
        fork: crate::ForkName,
    },

    /// The delta payload could not be decoded or does not contain the data
    /// required to apply it.
    ///
    /// This can represent malformed or corrupted serialized data, including
    /// invalid `rkyv` data or a failed `zstd` decompression.
    MalformedDelta(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForkMismatch {
                state_fork,
                delta_fork,
            } => {
                write!(
                    f,
                    "Fork mismatch: cannot apply {delta_fork:?} delta to {state_fork:?} state",
                )
            }
            Self::InvalidFieldForFork { field, fork } => {
                write!(f, "Field '{field}' is invalid for fork {fork:?}")
            }
            Self::MalformedDelta(message) => {
                write!(f, "Malformed delta payload: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}
