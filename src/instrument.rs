//! Instrument-publication lifecycle semantics.
//!
//! Published instrument releases are immutable artifacts. Lifecycle commands
//! control whether the same immutable release may begin new sessions; content
//! changes require a distinct instrument-version artifact rather than a state
//! mutation of an existing published release.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Server-authoritative lifecycle state for one immutable instrument release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationState {
    /// The release is being authored and cannot accept assessment sessions.
    Draft,
    /// The release is awaiting governance, scientific, or product review.
    Review,
    /// The immutable release may accept new assessment sessions.
    Published,
    /// New sessions are temporarily blocked without changing release bytes.
    Suspended,
    /// New sessions are permanently blocked for this release version.
    Retired,
}

impl PublicationState {
    /// Return whether this exact release may accept new assessment sessions.
    #[must_use]
    pub const fn accepts_new_sessions(self) -> bool {
        matches!(self, Self::Published)
    }

    /// Return whether no further lifecycle state other than itself is allowed.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Retired)
    }
}

/// A command requesting one legal instrument-publication transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationCommand {
    /// Submit a draft release for review.
    SubmitReview,
    /// Publish a reviewed immutable release.
    Publish,
    /// Temporarily block new sessions for a published release.
    Suspend,
    /// Resume a suspended release without changing its immutable contents.
    Reactivate,
    /// Permanently block new sessions for this release version.
    Retire,
}

/// Fail-closed error returned for an undocumented publication transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationError {
    /// The requested command is not legal from the current publication state.
    InvalidTransition {
        /// State in which the rejected command was attempted.
        state: PublicationState,
        /// Command rejected by the lifecycle contract.
        command: PublicationCommand,
    },
}

impl PublicationError {
    /// Return the publication state in which the error occurred.
    #[must_use]
    pub const fn state(self) -> PublicationState {
        match self {
            Self::InvalidTransition { state, .. } => state,
        }
    }

    /// Return the rejected publication command.
    #[must_use]
    pub const fn command(self) -> PublicationCommand {
        match self {
            Self::InvalidTransition { command, .. } => command,
        }
    }
}

impl Display for PublicationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTransition { state, command } => write!(
                formatter,
                "command {command:?} is not valid while instrument release is {state:?}"
            ),
        }
    }
}

impl Error for PublicationError {}

/// Apply a publication command to one immutable instrument release.
///
/// Repeating a command that merely confirms an already-applied lifecycle state
/// is idempotent. Reactivation is distinct from publishing: it only resumes the
/// same suspended immutable release. Any content or policy change must produce
/// a new instrument-version artifact outside this state machine.
///
/// # Errors
///
/// Returns [`PublicationError::InvalidTransition`] for every undocumented
/// source-state and command combination.
pub const fn transition(
    state: PublicationState,
    command: PublicationCommand,
) -> Result<PublicationState, PublicationError> {
    use PublicationCommand::{Publish, Reactivate, Retire, SubmitReview, Suspend};
    use PublicationState::{Draft, Published, Retired, Review, Suspended};

    let next = match (state, command) {
        (Draft | Review, SubmitReview) => Review,
        (Review | Published, Publish) => Published,
        (Published | Suspended, Suspend) => Suspended,
        (Published | Suspended, Reactivate) => Published,
        (Published | Suspended | Retired, Retire) => Retired,
        _ => return Err(PublicationError::InvalidTransition { state, command }),
    };

    Ok(next)
}
