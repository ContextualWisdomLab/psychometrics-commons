//! Assessment-session lifecycle semantics.
//!
//! Session state is server-authoritative. Clients submit commands and the
//! runtime decides whether a transition is legal; clients never submit an
//! arbitrary target state. Duplicate commands that represent the transition
//! already applied are idempotent. Session creation pins the exact immutable
//! published instrument release and locale before lifecycle transitions begin.

use crate::instrument::InstrumentRelease;
use crate::reference::normalized_reference;
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

/// Fail-closed error returned while creating a session from a published release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionCreationError {
    /// A session or participant reference was blank or numeric-like.
    InvalidReference,
    /// The server-authoritative session creation timestamp was zero.
    InvalidTimestamp,
    /// The selected immutable release is not currently allowed to begin new sessions.
    InstrumentReleaseUnavailable,
    /// The requested assessment locale does not exactly match the published release locale.
    LocaleMismatch,
}

impl Display for SessionCreationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "assessment session references must be opaque non-numeric values"
            }
            Self::InvalidTimestamp => "assessment session creation time must be greater than zero",
            Self::InstrumentReleaseUnavailable => {
                "assessment session requires an instrument release currently published for new sessions"
            }
            Self::LocaleMismatch => {
                "assessment session locale must exactly match the published instrument release locale"
            }
        })
    }
}

impl Error for SessionCreationError {}

/// Immutable creation identity and current lifecycle state for one assessment session.
///
/// The session copies only release/version/locale identity from an already-validated
/// [`InstrumentRelease`]. It does not duplicate instrument publication evidence rules.
/// Suspending or retiring the release later blocks *new* sessions but does not rewrite
/// the provenance of a session that was validly created while the release was published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssessmentSession {
    session_ref: String,
    participant_ref: String,
    instrument_release_ref: String,
    instrument_version_ref: String,
    locale: String,
    created_at_unix_ms: u64,
    state: SessionState,
}

impl AssessmentSession {
    /// Create a session bound to one exact published locale-specific instrument release.
    ///
    /// This boundary enforces the no-silent-assessment-locale-fallback contract. Callers
    /// must resolve a release whose exact locale matches the participant-requested locale;
    /// the runtime does not substitute another published language.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCreationError::InvalidReference`] for malformed session/participant
    /// references, [`SessionCreationError::InvalidTimestamp`] for a zero server timestamp,
    /// [`SessionCreationError::InstrumentReleaseUnavailable`] unless the exact release can
    /// currently accept new sessions, or [`SessionCreationError::LocaleMismatch`] when the
    /// requested locale is not exactly the release locale.
    pub fn new(
        session_ref: &str,
        participant_ref: &str,
        release: &InstrumentRelease,
        requested_locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, SessionCreationError> {
        let session_ref =
            normalized_reference(session_ref).ok_or(SessionCreationError::InvalidReference)?;
        let participant_ref =
            normalized_reference(participant_ref).ok_or(SessionCreationError::InvalidReference)?;
        if created_at_unix_ms == 0 {
            return Err(SessionCreationError::InvalidTimestamp);
        }
        if !release.accepts_new_sessions() {
            return Err(SessionCreationError::InstrumentReleaseUnavailable);
        }
        if requested_locale != release.manifest().locale() {
            return Err(SessionCreationError::LocaleMismatch);
        }

        Ok(Self {
            session_ref: session_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            instrument_release_ref: release.manifest().release_ref().to_owned(),
            instrument_version_ref: release.manifest().instrument_version_ref().to_owned(),
            locale: release.manifest().locale().to_owned(),
            created_at_unix_ms,
            state: SessionState::Created,
        })
    }

    /// Return this session's opaque product reference.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the stable participant reference without exposing external identity subjects.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the immutable instrument-release reference pinned at creation.
    #[must_use]
    pub fn instrument_release_ref(&self) -> &str {
        &self.instrument_release_ref
    }

    /// Return the immutable instrument-version reference pinned at creation.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the exact assessment-content locale pinned at creation.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the server-authoritative creation timestamp in Unix milliseconds.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return the current server-authoritative lifecycle state.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Apply one lifecycle command to this aggregate's server-authoritative state.
    ///
    /// Creation provenance is immutable: only the lifecycle state changes. Duplicate
    /// commands retain the idempotency semantics of [`transition`].
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] when the command is not legal from the current state.
    pub fn apply_command(
        &mut self,
        command: SessionCommand,
    ) -> Result<SessionState, TransitionError> {
        let next = transition(self.state, command)?;
        self.state = next;
        Ok(next)
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
