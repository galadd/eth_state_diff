use std::fmt;

/// Errors that can occur during delta creation or application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The delta targets a different fork than the provided state.
    ForkMismatch {
        state_fork: crate::ForkName,
        delta_fork: crate::ForkName,
    },
    /// A field was included in the delta that does not exist for the delta's fork.
    InvalidFieldForFork {
        field: &'static str,
        fork: crate::ForkName,
    },
    /// The delta payload is malformed or missing expected data (e.g., corrupted zstd/rkyv).
    MalformedDelta(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ForkMismatch {
                state_fork,
                delta_fork,
            } => {
                write!(
                    f,
                    "Fork mismatch: cannot apply {delta_fork:?} delta to {state_fork:?} state",
                )
            }
            Error::InvalidFieldForFork { field, fork } => {
                write!(f, "Field '{field}' is invalid for fork {fork:?}")
            }
            Error::MalformedDelta(msg) => {
                write!(f, "Malformed delta payload: {msg}")
            }
        }
    }
}

impl std::error::Error for Error {}
