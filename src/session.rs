//! Assessment-session lifecycle semantics.
//!
//! Session state is server-authoritative. Clients submit commands and the
//! runtime decides whether a transition is legal; clients never submit an
//! arbitrary target state. Accepted commands carry server-issued replay identity
//! and ordering so an exact retransmission returns its original outcome without
//! rewinding later state. Session creation pins the exact immutable published
//! instrument release, content digest, and locale before lifecycle transitions begin.

use crate::instrument::{
    valid_locale, valid_sha256_digest, InstrumentRelease, InstrumentReleaseManifest,
};
use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

fn exact_reference(value: &str) -> Option<&str> {
    normalized_reference(value).filter(|normalized| *normalized == value)
}

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
    /// A session or participant reference was blank, numeric-like, or not exact.
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
                "assessment session references must use their exact opaque non-numeric spelling"
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

/// Fail-closed error returned while restoring a created session from stored identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SessionReconstitutionError {
    /// A session, participant, release, or version reference was blank or numeric-like.
    InvalidReference,
    /// The stored session creation timestamp was zero.
    InvalidTimestamp,
    /// The stored release content digest was not a canonical SHA-256 digest.
    InvalidContentDigest,
    /// The stored locale was not an exact whitespace-free BCP 47-style tag.
    InvalidLocale,
}

impl Display for SessionReconstitutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "use an opaque non-numeric session, participant, release, or version reference"
            }
            Self::InvalidTimestamp => "use a stored creation time greater than zero",
            Self::InvalidContentDigest => {
                "use a sha256 digest with 64 lowercase hexadecimal digits"
            }
            Self::InvalidLocale => "use an exact whitespace-free BCP 47-style locale tag",
        })
    }
}

impl Error for SessionReconstitutionError {}

/// One accepted server-authoritative session command.
///
/// Persist this history so a later load can replay Activate/Pause/Resume without
/// inventing a new lifecycle path. Exact replay identity is the command reference
/// plus sequence and command evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedSessionCommand {
    command_ref: String,
    sequence: u64,
    command: SessionCommand,
    resulting_state: SessionState,
}

impl AcceptedSessionCommand {
    /// Return the opaque server command reference.
    #[must_use]
    pub fn command_ref(&self) -> &str {
        &self.command_ref
    }

    /// Return the positive strictly increasing command sequence.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Return the lifecycle command that was accepted.
    #[must_use]
    pub const fn command(&self) -> SessionCommand {
        self.command
    }

    /// Return the state produced by this accepted command.
    #[must_use]
    pub const fn resulting_state(&self) -> SessionState {
        self.resulting_state
    }
}

/// Immutable creation identity and current lifecycle state for one assessment session.
///
/// The session copies release/version/content-digest/locale identity from an already-validated
/// [`InstrumentRelease`]. It does not duplicate instrument publication evidence rules.
/// Suspending or retiring the release later blocks *new* sessions but does not rewrite
/// the provenance of a session that was validly created while the release was published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssessmentSession {
    session_ref: String,
    participant_ref: String,
    instrument_release_ref: String,
    instrument_version_ref: String,
    instrument_release_content_digest: String,
    locale: String,
    created_at_unix_ms: u64,
    state: SessionState,
    accepted_commands: Vec<AcceptedSessionCommand>,
}

impl AssessmentSession {
    /// Create a session from one exact published locale-specific instrument release.
    ///
    /// Session and participant references must already use their exact accepted opaque spelling;
    /// surrounding whitespace is rejected rather than silently removed. `requested_locale` must
    /// exactly equal the locale stored on `release`. If they differ, session creation fails with
    /// [`SessionCreationError::LocaleMismatch`]. This method never substitutes another published
    /// locale or language.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCreationError::InvalidReference`] for malformed or non-exact
    /// session/participant references, [`SessionCreationError::InvalidTimestamp`] for a zero
    /// server timestamp, [`SessionCreationError::InstrumentReleaseUnavailable`] unless the exact
    /// release can currently accept new sessions, or [`SessionCreationError::LocaleMismatch`]
    /// when the requested locale is not exactly the release locale.
    pub fn new(
        session_ref: &str,
        participant_ref: &str,
        release: &InstrumentRelease,
        requested_locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, SessionCreationError> {
        let session_ref =
            exact_reference(session_ref).ok_or(SessionCreationError::InvalidReference)?;
        let participant_ref =
            exact_reference(participant_ref).ok_or(SessionCreationError::InvalidReference)?;
        if created_at_unix_ms == 0 {
            return Err(SessionCreationError::InvalidTimestamp);
        }
        if !release.accepts_new_sessions() {
            return Err(SessionCreationError::InstrumentReleaseUnavailable);
        }
        Self::from_currently_published_manifest(
            session_ref,
            participant_ref,
            release.manifest(),
            requested_locale,
            created_at_unix_ms,
        )
    }

    /// Create a session from a manifest that a published-release load already accepted.
    ///
    /// Use this after
    /// [`crate::postgres_instrument_release::load_published_instrument_release`].
    /// It does not re-check publication lifecycle; the load boundary is the
    /// eligibility gate. Call [`AssessmentSession::new`] when the caller still
    /// holds a live [`InstrumentRelease`]. Session and participant references must
    /// already use their exact accepted opaque spelling.
    ///
    /// # Errors
    ///
    /// Returns [`SessionCreationError::InvalidReference`] for malformed or non-exact
    /// session/participant references, [`SessionCreationError::InvalidTimestamp`]
    /// for a zero server timestamp, or [`SessionCreationError::LocaleMismatch`]
    /// when the requested locale is not exactly the manifest locale.
    pub fn from_currently_published_manifest(
        session_ref: &str,
        participant_ref: &str,
        manifest: &InstrumentReleaseManifest,
        requested_locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, SessionCreationError> {
        let session_ref =
            exact_reference(session_ref).ok_or(SessionCreationError::InvalidReference)?;
        let participant_ref =
            exact_reference(participant_ref).ok_or(SessionCreationError::InvalidReference)?;
        if created_at_unix_ms == 0 {
            return Err(SessionCreationError::InvalidTimestamp);
        }
        if requested_locale != manifest.locale() {
            return Err(SessionCreationError::LocaleMismatch);
        }

        Ok(Self {
            session_ref: session_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            instrument_release_ref: manifest.release_ref().to_owned(),
            instrument_version_ref: manifest.instrument_version_ref().to_owned(),
            instrument_release_content_digest: manifest.content_digest().to_owned(),
            locale: manifest.locale().to_owned(),
            created_at_unix_ms,
            state: SessionState::Created,
            accepted_commands: Vec::new(),
        })
    }

    /// Restore a created session from durable identity without a live published release.
    ///
    /// Use this after loading a stored created-session row. It does not re-check whether
    /// the original release still accepts new sessions, so a later suspend or retire
    /// cannot rewrite provenance. Command history starts empty here; load replays
    /// stored commands after this reconstitution. Call [`AssessmentSession::new`]
    /// when starting a new session.
    ///
    /// # Errors
    ///
    /// Returns [`SessionReconstitutionError`] when a stored reference, digest, locale,
    /// or creation timestamp is not a valid created-session identity.
    pub fn from_persisted_created(
        session_ref: &str,
        participant_ref: &str,
        instrument_release_ref: &str,
        instrument_version_ref: &str,
        instrument_release_content_digest: &str,
        locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, SessionReconstitutionError> {
        let session_ref = normalized_reference(session_ref)
            .ok_or(SessionReconstitutionError::InvalidReference)?;
        let participant_ref = normalized_reference(participant_ref)
            .ok_or(SessionReconstitutionError::InvalidReference)?;
        let instrument_release_ref = normalized_reference(instrument_release_ref)
            .ok_or(SessionReconstitutionError::InvalidReference)?;
        let instrument_version_ref = normalized_reference(instrument_version_ref)
            .ok_or(SessionReconstitutionError::InvalidReference)?;
        if created_at_unix_ms == 0 {
            return Err(SessionReconstitutionError::InvalidTimestamp);
        }
        if !valid_sha256_digest(instrument_release_content_digest) {
            return Err(SessionReconstitutionError::InvalidContentDigest);
        }
        if !valid_locale(locale) {
            return Err(SessionReconstitutionError::InvalidLocale);
        }

        Ok(Self {
            session_ref: session_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            instrument_release_ref: instrument_release_ref.to_owned(),
            instrument_version_ref: instrument_version_ref.to_owned(),
            instrument_release_content_digest: instrument_release_content_digest.to_owned(),
            locale: locale.to_owned(),
            created_at_unix_ms,
            state: SessionState::Created,
            accepted_commands: Vec::new(),
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

    /// Return the canonical content digest of the immutable release pinned at creation.
    #[must_use]
    pub fn instrument_release_content_digest(&self) -> &str {
        &self.instrument_release_content_digest
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

    /// Return accepted command history in sequence order.
    ///
    /// Use this when persisting later lifecycle states. Load reconstitutes a
    /// created session and replays these commands; it does not invent state.
    #[must_use]
    pub fn accepted_commands(&self) -> &[AcceptedSessionCommand] {
        &self.accepted_commands
    }

    /// Apply one identified lifecycle command to this aggregate's server-authoritative state.
    ///
    /// New commands must carry a normalized opaque server command reference and a positive,
    /// strictly increasing sequence. An exact replay of an accepted command returns the state
    /// produced by that original command but does not mutate the aggregate's current state.
    /// Reusing a command reference with different sequence or command evidence fails closed.
    /// Creation provenance never changes.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] for malformed command identity, invalid ordering,
    /// conflicting replay evidence, or a lifecycle command not legal from the current state.
    pub fn apply_command(
        &mut self,
        command_ref: &str,
        sequence: u64,
        command: SessionCommand,
    ) -> Result<SessionState, TransitionError> {
        let command_ref = normalized_reference(command_ref).ok_or_else(|| {
            TransitionError::new(self.state, command, TransitionErrorKind::InvalidReference)
        })?;

        if let Some(accepted) = self
            .accepted_commands
            .iter()
            .find(|accepted| accepted.command_ref == command_ref)
        {
            return if accepted.sequence == sequence && accepted.command == command {
                Ok(accepted.resulting_state)
            } else {
                Err(TransitionError::new(
                    self.state,
                    command,
                    TransitionErrorKind::ConflictingReplay,
                ))
            };
        }

        if sequence == 0
            || self
                .accepted_commands
                .last()
                .is_some_and(|accepted| sequence <= accepted.sequence)
        {
            return Err(TransitionError::new(
                self.state,
                command,
                TransitionErrorKind::InvalidSequence,
            ));
        }

        let next = transition(self.state, command)?;
        self.state = next;
        self.accepted_commands.push(AcceptedSessionCommand {
            command_ref: command_ref.to_owned(),
            sequence,
            command,
            resulting_state: next,
        });
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
    /// Make a scored result available according to the product access policy.
    Release,
    /// Expire a pre-completion session according to its publication policy.
    Expire,
    /// Cancel a pre-completion session.
    Cancel,
    /// Invalidate a session whose evidence must not proceed to normal serving.
    Invalidate,
}

impl SessionCommand {
    /// Return the stable persisted vocabulary for this command.
    #[must_use]
    pub const fn persist_name(self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Complete => "complete",
            Self::BeginScoring => "begin_scoring",
            Self::RecordScore => "record_score",
            Self::Release => "release",
            Self::Expire => "expire",
            Self::Cancel => "cancel",
            Self::Invalidate => "invalidate",
        }
    }

    /// Parse a persisted command name.
    #[must_use]
    pub fn from_persist_name(name: &str) -> Option<Self> {
        match name {
            "activate" => Some(Self::Activate),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "complete" => Some(Self::Complete),
            "begin_scoring" => Some(Self::BeginScoring),
            "record_score" => Some(Self::RecordScore),
            "release" => Some(Self::Release),
            "expire" => Some(Self::Expire),
            "cancel" => Some(Self::Cancel),
            "invalidate" => Some(Self::Invalidate),
            _ => None,
        }
    }
}

impl SessionState {
    /// Return the stable persisted vocabulary for this state.
    #[must_use]
    pub const fn persist_name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Scoring => "scoring",
            Self::Scored => "scored",
            Self::Released => "released",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Invalidated => "invalidated",
        }
    }

    /// Parse a persisted session-state name.
    #[must_use]
    pub fn from_persist_name(name: &str) -> Option<Self> {
        match name {
            "created" => Some(Self::Created),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "scoring" => Some(Self::Scoring),
            "scored" => Some(Self::Scored),
            "released" => Some(Self::Released),
            "expired" => Some(Self::Expired),
            "cancelled" => Some(Self::Cancelled),
            "invalidated" => Some(Self::Invalidated),
            _ => None,
        }
    }
}

/// Stable reason category for a rejected identified session command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TransitionErrorKind {
    /// The server command reference was blank or numeric-like rather than opaque.
    InvalidReference,
    /// The server command sequence was zero or did not advance beyond accepted commands.
    InvalidSequence,
    /// An accepted command reference was replayed with different immutable evidence.
    ConflictingReplay,
    /// The lifecycle command is not valid from the aggregate's current state.
    InvalidTransition,
}

/// Error returned when an identified command cannot be safely applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionError {
    state: SessionState,
    command: SessionCommand,
    kind: TransitionErrorKind,
}

impl TransitionError {
    const fn new(state: SessionState, command: SessionCommand, kind: TransitionErrorKind) -> Self {
        Self {
            state,
            command,
            kind,
        }
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

    /// Return the stable rejection category.
    #[must_use]
    pub const fn kind(self) -> TransitionErrorKind {
        self.kind
    }
}

impl Display for TransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            TransitionErrorKind::InvalidReference => {
                formatter.write_str("session command reference must be opaque and non-numeric")
            }
            TransitionErrorKind::InvalidSequence => formatter
                .write_str("session command sequence must be positive and strictly increasing"),
            TransitionErrorKind::ConflictingReplay => formatter
                .write_str("session command reference was replayed with conflicting evidence"),
            TransitionErrorKind::InvalidTransition => write!(
                formatter,
                "command {:?} is not valid while session is {:?}",
                self.command, self.state
            ),
        }
    }
}

impl Error for TransitionError {}

/// Apply a session command to a server-authoritative lifecycle state.
///
/// Duplicate commands that merely confirm an already-applied transition are
/// idempotent at this state-only helper. Aggregate-level replay across intervening
/// commands is enforced by [`AssessmentSession::apply_command`]. Other illegal
/// transitions fail closed and preserve the source state in [`TransitionError`].
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
        _ => {
            return Err(TransitionError::new(
                state,
                command,
                TransitionErrorKind::InvalidTransition,
            ));
        }
    };

    Ok(next)
}
