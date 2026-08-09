//! Assessment-session lifecycle semantics.
//!
//! Session state is server-authoritative. Clients submit commands and the
//! runtime decides whether a transition is legal; clients never submit an
//! arbitrary target state. Duplicate commands that represent the transition
//! already applied are idempotent.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Server-authoritative lifecycle state for one assessment session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionState {
    /// The session exists but has not begun accepting responses.
    Created,
    /// The session may accept response events.
    Active,
    /// The participant intentionally paused the session.
    Paused,
    /// Response collection is closed and an immutable snapshot can be frozen.
    Completed,
    /// The frozen response snapshot is awaiting or undergoing scoring.
    Scoring,
    /// A valid immutable scoring result exists.
    Scored,
    /// The result has been made available through the product access policy.
    Released,
    /// The session expired before completion under its publication policy.
    Expired,
    /// The participant or an authorized product workflow cancelled the session.
    Cancelled,
    /// The session was invalidated because its evidence must not be scored or served.
    Invalidated,
}

impl SessionState {
    /// Return whether the session is in a terminal lifecycle state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Released | Self::Expired | Self::Cancelled | Self::Invalidated
        )
    }

    /// Return whether new response events may be accepted for this session.
    #[must_use]
    pub const fn accepts_responses(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// A command requesting one legal session lifecycle transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionCommand {
    /// Start a newly created session, or confirm that it is already active.
    Activate,
    /// Pause an active session, or confirm that it is already paused.
    Pause,
    /// Resume a paused session, or confirm that it is already active.
    Resume,
    /// Close response collection and freeze the response set for snapshotting.
    Complete,
    /// Begin asynchronous scoring from the frozen response snapshot.
    BeginScoring,
    /// Record successful completion of the scoring operation.
    RecordScore,
    /// Make a scored result available according to the result access policy.
    Release,
    /// Expire a pre-completion session according to its publication policy.
    Expire,
    /// Cancel a pre-completion session.
    Cancel,
    /// Invalidate a session whose evidence must not proceed to normal serving.
    Invalidate,
}

/// Error returned when a command is not legal for the current session state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionError {
    state: SessionState,
    command: SessionCommand,
}

impl TransitionError {
    const fn new(state: SessionState, command: SessionCommand) -> Self {
        Self { state, command }
    }

    /// Return the state in which the rejected command was attempted.
    #[must_use]
    pub const fn state(self) -> SessionState {
        self.state
    }

    /// Return the rejected command.
    #[must_use]
    pub const fn command(self) -> SessionCommand {
        self.command
    }
}

impl Display for TransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "command {:?} is not valid while session is {:?}",
            self.command, self.state
        )
    }
}

impl Error for TransitionError {}

/// Apply a session command to a server-authoritative lifecycle state.
///
/// Duplicate commands that merely confirm an already-applied transition are
/// idempotent. Other illegal transitions fail closed and preserve the source
/// state in [`TransitionError`].
///
/// # Errors
///
/// Returns [`TransitionError`] when `command` is not legal from `state`.
pub const fn transition(
    state: SessionState,
    command: SessionCommand,
) -> Result<SessionState, TransitionError> {
    use SessionCommand::{
        Activate, BeginScoring, Cancel, Complete, Expire, Invalidate, Pause, RecordScore, Release,
        Resume,
    };
    use SessionState::{
        Active, Cancelled, Completed, Created, Expired, Invalidated, Paused, Released, Scored,
        Scoring,
    };

    let next = match (state, command) {
        (Created | Active, Activate) | (Paused | Active, Resume) => Active,
        (Active | Paused, Pause) => Paused,
        (Active | Completed, Complete) => Completed,
        (Completed | Scoring, BeginScoring) => Scoring,
        (Scoring | Scored, RecordScore) => Scored,
        (Scored | Released, Release) => Released,
        (Created | Active | Paused | Cancelled, Cancel) => Cancelled,
        (Created | Active | Paused | Expired, Expire) => Expired,
        (Created | Active | Paused | Completed | Scoring | Scored | Invalidated, Invalidate) => {
            Invalidated
        }
        _ => return Err(TransitionError::new(state, command)),
    };

    Ok(next)
}
