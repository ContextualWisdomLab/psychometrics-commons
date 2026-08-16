//! `PostgreSQL` 18 persistence for assessment-session identity and command history.
//!
//! This module stores the participant, published-release, version, content-digest,
//! and locale identity copied at session creation. It does not rewrite provenance
//! when the release is later suspended or retired. Created identity is inserted
//! only for [`SessionState::Created`]. Later lifecycle states persist as
//! append-only command history plus a current-state projection. A shorter
//! persist than already stored fails closed so a stale worker cannot rewind
//! that projection. Command persist locks the session header row until the
//! caller transaction ends so a concurrent Activate-only worker cannot
//! overwrite a later Pause/Resume after that later command commits. Load
//! restores created identity without re-checking publication eligibility,
//! then replays stored commands. Replay requires `READ COMMITTED`.

use crate::instrument::InstrumentRelease;
use crate::reference::normalized_reference;
use crate::session::{
    created_session_for_start, AcceptedSessionCommand, AssessmentSession, SessionCommand,
    SessionCreationError, SessionState,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const ASSESSMENT_SESSION_MIGRATION: &str =
    include_str!("../migrations/0014_assessment_session.sql");
const ASSESSMENT_SESSION_COMMAND_MIGRATION: &str =
    include_str!("../migrations/0016_assessment_session_command.sql");

/// Outcome of persisting one created assessment session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AssessmentSessionPersistenceDisposition {
    /// A new assessment-session row was inserted.
    Inserted,
    /// The same immutable created-session identity already existed.
    Duplicate,
}

/// Fail-closed error for durable assessment-session persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssessmentSessionPersistenceError {
    /// A creation timestamp cannot be represented by the bounded database column.
    ValueOutOfRange,
    /// Only a newly created session can be inserted by this first persist slice.
    UnsupportedInitialState,
    /// Session identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// Assessment-session persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// A session reference used for load was blank or numeric-like.
    InvalidReference,
    /// Stored session identity could not be restored as a created session.
    InvalidStoredIdentity,
    /// Stored session state is not created and has no command history to replay.
    UnsupportedStoredState,
    /// Later commands were persisted before the created-session identity row.
    MissingCreatedIdentity,
    /// A command sequence was reused by a different command identity.
    SequenceConflict,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for AssessmentSessionPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ValueOutOfRange => {
                "assessment session persistence value exceeds the PostgreSQL range"
            }
            Self::UnsupportedInitialState => {
                "only a newly created assessment session may be inserted"
            }
            Self::ConflictingReplay => {
                "assessment session identity was replayed with conflicting evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "assessment session persistence requires read committed isolation"
            }
            Self::InvalidReference => {
                "use an opaque non-numeric session reference to load a stored session"
            }
            Self::InvalidStoredIdentity => {
                "stored assessment-session identity could not be restored; repair the row or persist a valid created session"
            }
            Self::UnsupportedStoredState => {
                "load a created assessment session; persist later lifecycle states before loading them"
            }
            Self::MissingCreatedIdentity => {
                "persist the created assessment session before persisting later commands"
            }
            Self::SequenceConflict => {
                "session command sequence was reused by a different command identity"
            }
            Self::Database(_) => "PostgreSQL assessment-session persistence failed",
        })
    }
}

impl Error for AssessmentSessionPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for AssessmentSessionPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Fail-closed error for starting and persisting a created assessment session.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssessmentSessionStartError {
    /// The release is unpublished, the locale does not match, or identity is invalid.
    Creation(SessionCreationError),
    /// Durable persist or load rejected the start attempt.
    Persistence(AssessmentSessionPersistenceError),
}

impl Display for AssessmentSessionStartError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creation(error) => Display::fmt(error, formatter),
            Self::Persistence(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for AssessmentSessionStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Creation(error) => Some(error),
            Self::Persistence(error) => Some(error),
        }
    }
}

impl From<SessionCreationError> for AssessmentSessionStartError {
    fn from(error: SessionCreationError) -> Self {
        Self::Creation(error)
    }
}

impl From<AssessmentSessionPersistenceError> for AssessmentSessionStartError {
    fn from(error: AssessmentSessionPersistenceError) -> Self {
        Self::Persistence(error)
    }
}

/// Apply the idempotent assessment-session and command-history migrations.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_assessment_session_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ASSESSMENT_SESSION_MIGRATION)?;
    client.batch_execute(ASSESSMENT_SESSION_COMMAND_MIGRATION)
}

/// Persist one created assessment-session identity bound to a published release.
///
/// Exact replay of the same session, participant, release, `instrument_version_ref`,
/// digest, locale, state, and creation time is idempotent. Rebinding any stored
/// field fails closed. [`AssessmentSession::new`] validates and normalizes session
/// and participant references. This function stores those references without
/// validating them again.
///
/// # Errors
///
/// Returns [`AssessmentSessionPersistenceError`] for unsupported isolation,
/// a non-created session, conflicting replay, an out-of-range timestamp,
/// or a database failure.
pub fn persist_assessment_session(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
) -> Result<AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError> {
    require_read_committed(transaction)?;
    if session.state() != SessionState::Created {
        return Err(AssessmentSessionPersistenceError::UnsupportedInitialState);
    }
    let session_ref = session.session_ref();
    let participant_ref = session.participant_ref();
    let created_at_unix_ms = postgres_bigint(session.created_at_unix_ms())?;
    let session_state = session.state().persist_name();
    let inserted = transaction.execute(
        "INSERT INTO assessment_session (
             session_ref, participant_ref, instrument_release_ref,
             instrument_version_ref, instrument_release_content_digest, locale,
             session_state, created_at_unix_ms
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (session_ref) DO NOTHING",
        &[
            &session_ref,
            &participant_ref,
            &session.instrument_release_ref(),
            &session.instrument_version_ref(),
            &session.instrument_release_content_digest(),
            &session.locale(),
            &session_state,
            &created_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        return Ok(AssessmentSessionPersistenceDisposition::Inserted);
    }
    classify_existing_session(transaction, session, session_ref, created_at_unix_ms)
}

/// Start a created session only while the release still accepts new sessions.
///
/// This is the persist boundary HTTP and other transports must call. It runs
/// [`created_session_for_start`] so a suspended or retired release cannot mint a
/// reconstituted identity and then persist it. Exact replay of an already stored
/// start returns the original session even after a later suspend, so a buyer who
/// already started can retry without losing the session.
///
/// # Errors
///
/// Returns [`AssessmentSessionStartError::Creation`] when the release cannot
/// accept a new session and no exact stored start exists, or
/// [`AssessmentSessionStartError::Persistence`] for conflicting stored identity
/// or a database failure.
pub fn start_created_assessment_session(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<(AssessmentSession, AssessmentSessionPersistenceDisposition), AssessmentSessionStartError>
{
    match created_session_for_start(
        session_ref,
        participant_ref,
        release,
        requested_locale,
        created_at_unix_ms,
    ) {
        Ok(session) => {
            let disposition = persist_assessment_session(transaction, &session)?;
            Ok((session, disposition))
        }
        Err(SessionCreationError::InstrumentReleaseUnavailable) => {
            replay_started_session_after_publication_block(
                transaction,
                session_ref,
                participant_ref,
                release,
                requested_locale,
                created_at_unix_ms,
            )
        }
        Err(error) => Err(AssessmentSessionStartError::Creation(error)),
    }
}

/// Return an exact stored start after the release no longer accepts new sessions.
fn replay_started_session_after_publication_block(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<(AssessmentSession, AssessmentSessionPersistenceDisposition), AssessmentSessionStartError>
{
    let Some(stored) = load_assessment_session(transaction, session_ref)? else {
        return Err(AssessmentSessionStartError::Creation(
            SessionCreationError::InstrumentReleaseUnavailable,
        ));
    };
    if stored_start_identity_matches(
        &stored,
        session_ref,
        participant_ref,
        release,
        requested_locale,
        created_at_unix_ms,
    ) {
        Ok((stored, AssessmentSessionPersistenceDisposition::Duplicate))
    } else {
        Err(AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay,
        ))
    }
}

/// Compare a stored session with the start request without re-checking publication.
fn stored_start_identity_matches(
    stored: &AssessmentSession,
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> bool {
    let Some(session_ref) = normalized_reference(session_ref) else {
        return false;
    };
    let Some(participant_ref) = normalized_reference(participant_ref) else {
        return false;
    };
    stored.session_ref() == session_ref
        && stored.participant_ref() == participant_ref
        && stored.instrument_release_ref() == release.manifest().release_ref()
        && stored.instrument_version_ref() == release.manifest().instrument_version_ref()
        && stored.instrument_release_content_digest() == release.manifest().content_digest()
        && stored.locale() == requested_locale
        && stored.created_at_unix_ms() == created_at_unix_ms
}

/// Persist accepted command history and the current lifecycle-state projection.
///
/// The created-session identity row must already exist. Exact replay of the same
/// command reference, sequence, command, and resulting state is idempotent.
/// Rebinding command evidence, reusing a sequence under another command
/// identity, or persisting a shorter history than already stored fails closed so
/// a stale Activate-only worker cannot rewind Pause/Resume. The created-session
/// header row is locked for the caller transaction so a concurrent shorter
/// persist cannot overwrite a later projection after that later command
/// commits. Load later reconstitutes created identity and replays these
/// commands.
///
/// # Errors
///
/// Returns [`AssessmentSessionPersistenceError`] for unsupported isolation, a
/// missing created-session row, conflicting replay, a sequence conflict, an
/// out-of-range sequence, or a database failure.
pub fn persist_assessment_session_commands(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
) -> Result<AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError> {
    require_read_committed(transaction)?;
    require_existing_created_identity(transaction, session)?;
    let session_ref = session.session_ref();
    let mut inserted_any = false;
    for command in session.accepted_commands() {
        if persist_one_session_command(transaction, session_ref, command)? {
            inserted_any = true;
        }
    }
    let stored_command_count: i64 = transaction
        .query_one(
            "SELECT COUNT(*) FROM assessment_session_command WHERE session_ref = $1",
            &[&session_ref],
        )?
        .get(0);
    reject_stale_command_prefix(stored_command_count, session.accepted_commands().len())?;
    transaction.execute(
        "UPDATE assessment_session SET session_state = $2 WHERE session_ref = $1",
        &[&session_ref, &session.state().persist_name()],
    )?;
    Ok(if inserted_any {
        AssessmentSessionPersistenceDisposition::Inserted
    } else {
        AssessmentSessionPersistenceDisposition::Duplicate
    })
}

/// Load one created assessment-session identity without a live published release.
///
/// Missing rows return [`None`]. A stored later lifecycle state without command
/// history fails closed. Exact stored identity is restored through
/// [`AssessmentSession::from_persisted_created`], then accepted commands are
/// replayed, so a later suspend or retire cannot rewrite provenance.
///
/// # Errors
///
/// Returns [`AssessmentSessionPersistenceError`] for unsupported isolation, a
/// malformed session reference, a non-created stored state, invalid stored
/// identity, or a database failure.
pub fn load_assessment_session(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<AssessmentSession>, AssessmentSessionPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = normalized_reference(session_ref)
        .ok_or(AssessmentSessionPersistenceError::InvalidReference)?;
    let row = match transaction.query_opt(
        "SELECT participant_ref, instrument_release_ref, instrument_version_ref,
                instrument_release_content_digest, locale, session_state,
                created_at_unix_ms
         FROM assessment_session WHERE session_ref = $1",
        &[&session_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_state: String = row.get(5);
    let stored_created_at: i64 = row.get(6);
    let created_at_unix_ms = u64::try_from(stored_created_at)
        .map_err(|_| AssessmentSessionPersistenceError::ValueOutOfRange)?;
    let commands = transaction.query(
        "SELECT command_ref, command_sequence, command_name, resulting_state
         FROM assessment_session_command
         WHERE session_ref = $1
         ORDER BY command_sequence",
        &[&session_ref],
    )?;
    if commands.is_empty() && stored_state != SessionState::Created.persist_name() {
        return Err(AssessmentSessionPersistenceError::UnsupportedStoredState);
    }
    let mut session = AssessmentSession::from_persisted_created(
        session_ref,
        row.get::<_, String>(0).as_str(),
        row.get::<_, String>(1).as_str(),
        row.get::<_, String>(2).as_str(),
        row.get::<_, String>(3).as_str(),
        row.get::<_, String>(4).as_str(),
        created_at_unix_ms,
    )
    .map_err(|_| AssessmentSessionPersistenceError::InvalidStoredIdentity)?;
    for command_row in commands {
        let command_ref: String = command_row.get(0);
        let stored_sequence: i64 = command_row.get(1);
        let command_name: String = command_row.get(2);
        let resulting_state: String = command_row.get(3);
        let sequence = u64::try_from(stored_sequence)
            .map_err(|_| AssessmentSessionPersistenceError::ValueOutOfRange)?;
        let command = SessionCommand::from_persist_name(&command_name)
            .ok_or(AssessmentSessionPersistenceError::InvalidStoredIdentity)?;
        let applied = session
            .apply_command(&command_ref, sequence, command)
            .map_err(|_| AssessmentSessionPersistenceError::InvalidStoredIdentity)?;
        if applied.persist_name() != resulting_state {
            return Err(AssessmentSessionPersistenceError::InvalidStoredIdentity);
        }
    }
    if session.state().persist_name() != stored_state {
        return Err(AssessmentSessionPersistenceError::InvalidStoredIdentity);
    }
    Ok(Some(session))
}

/// Compare an existing stored row with the requested immutable session identity.
///
/// Exact equality is treated as an idempotent duplicate; any changed participant,
/// release, `instrument_version_ref`, digest, locale, state, or creation time is a
/// conflicting replay.
fn classify_existing_session(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
    session_ref: &str,
    created_at_unix_ms: i64,
) -> Result<AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError> {
    let row = match transaction.query_one(
        "SELECT participant_ref, instrument_release_ref, instrument_version_ref,
                instrument_release_content_digest, locale, session_state,
                created_at_unix_ms
         FROM assessment_session WHERE session_ref = $1",
        &[&session_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
    let stored_participant: String = row.get(0);
    let stored_release: String = row.get(1);
    let stored_version: String = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_locale: String = row.get(4);
    let stored_state: String = row.get(5);
    let stored_created_at: i64 = row.get(6);
    if stored_participant == session.participant_ref()
        && stored_release == session.instrument_release_ref()
        && stored_version == session.instrument_version_ref()
        && stored_digest == session.instrument_release_content_digest()
        && stored_locale == session.locale()
        && stored_state == session.state().persist_name()
        && stored_created_at == created_at_unix_ms
    {
        Ok(AssessmentSessionPersistenceDisposition::Duplicate)
    } else {
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    }
}

/// Convert an unsigned millisecond timestamp into the database `BIGINT` range.
fn postgres_bigint(value: u64) -> Result<i64, AssessmentSessionPersistenceError> {
    i64::try_from(value).map_err(|_| AssessmentSessionPersistenceError::ValueOutOfRange)
}

/// Require and lock the created-session identity row before persisting later commands.
///
/// `FOR UPDATE` serializes command persist against the same `session_ref` so a
/// stale shorter history cannot count, then overwrite `session_state`, after a
/// later command has already committed.
fn require_existing_created_identity(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
) -> Result<(), AssessmentSessionPersistenceError> {
    let row = transaction
        .query_opt(
            "SELECT participant_ref, instrument_release_ref, instrument_version_ref,
                    instrument_release_content_digest, locale, created_at_unix_ms
             FROM assessment_session WHERE session_ref = $1
             FOR UPDATE",
            &[&session.session_ref()],
        )?
        .ok_or(AssessmentSessionPersistenceError::MissingCreatedIdentity)?;
    let stored_participant: String = row.get(0);
    let stored_release: String = row.get(1);
    let stored_version: String = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_locale: String = row.get(4);
    let stored_created_at: i64 = row.get(5);
    let created_at_unix_ms = postgres_bigint(session.created_at_unix_ms())?;
    if stored_participant == session.participant_ref()
        && stored_release == session.instrument_release_ref()
        && stored_version == session.instrument_version_ref()
        && stored_digest == session.instrument_release_content_digest()
        && stored_locale == session.locale()
        && stored_created_at == created_at_unix_ms
    {
        Ok(())
    } else {
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    }
}

/// Persist one accepted command, classifying exact or conflicting replay.
fn persist_one_session_command(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    command: &AcceptedSessionCommand,
) -> Result<bool, AssessmentSessionPersistenceError> {
    let command_sequence = postgres_bigint(command.sequence())?;
    let existing_sequence = transaction.query_opt(
        "SELECT command_ref FROM assessment_session_command
         WHERE session_ref = $1 AND command_sequence = $2",
        &[&session_ref, &command_sequence],
    )?;
    if let Some(row) = existing_sequence {
        let stored_ref: String = row.get(0);
        if stored_ref != command.command_ref() {
            return Err(AssessmentSessionPersistenceError::SequenceConflict);
        }
    }
    let inserted = transaction.execute(
        "INSERT INTO assessment_session_command (
             session_ref, command_ref, command_sequence, command_name, resulting_state
         ) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (session_ref, command_ref) DO NOTHING",
        &[
            &session_ref,
            &command.command_ref(),
            &command_sequence,
            &command.command().persist_name(),
            &command.resulting_state().persist_name(),
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }
    let row = transaction.query_one(
        "SELECT command_sequence, command_name, resulting_state
         FROM assessment_session_command
         WHERE session_ref = $1 AND command_ref = $2",
        &[&session_ref, &command.command_ref()],
    )?;
    let stored_sequence: i64 = row.get(0);
    let stored_name: String = row.get(1);
    let stored_result: String = row.get(2);
    if stored_sequence == command_sequence
        && stored_name == command.command().persist_name()
        && stored_result == command.resulting_state().persist_name()
    {
        Ok(false)
    } else {
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    }
}

/// Reject a persist that would rewind stored command history.
///
/// A stale worker that only remembers Activate must not overwrite a later
/// Pause/Resume projection. Count the already-stored rows after exact replay
/// classification; a shorter in-memory history is conflicting evidence.
fn reject_stale_command_prefix(
    stored_command_count: i64,
    accepted_command_count: usize,
) -> Result<(), AssessmentSessionPersistenceError> {
    let stored = u64::try_from(stored_command_count)
        .map_err(|_| AssessmentSessionPersistenceError::ValueOutOfRange)?;
    if stored > accepted_command_count as u64 {
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    } else {
        Ok(())
    }
}

/// Require the transaction isolation level used by the replay-classification contract.
fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), AssessmentSessionPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(AssessmentSessionPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        reject_stale_command_prefix, stored_start_identity_matches,
        AssessmentSessionPersistenceError, AssessmentSessionStartError,
    };
    use crate::instrument::{
        InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
        PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    };
    use crate::session::{AssessmentSession, SessionCommand, SessionCreationError, SessionState};

    #[test]
    fn session_persistence_errors_are_safe_and_specific() {
        for (error, expected) in [
            (
                AssessmentSessionPersistenceError::ValueOutOfRange,
                "assessment session persistence value exceeds the PostgreSQL range",
            ),
            (
                AssessmentSessionPersistenceError::UnsupportedInitialState,
                "only a newly created assessment session may be inserted",
            ),
            (
                AssessmentSessionPersistenceError::ConflictingReplay,
                "assessment session identity was replayed with conflicting evidence",
            ),
            (
                AssessmentSessionPersistenceError::UnsupportedIsolationLevel,
                "assessment session persistence requires read committed isolation",
            ),
            (
                AssessmentSessionPersistenceError::InvalidReference,
                "use an opaque non-numeric session reference to load a stored session",
            ),
            (
                AssessmentSessionPersistenceError::InvalidStoredIdentity,
                "stored assessment-session identity could not be restored; repair the row or persist a valid created session",
            ),
            (
                AssessmentSessionPersistenceError::UnsupportedStoredState,
                "load a created assessment session; persist later lifecycle states before loading them",
            ),
            (
                AssessmentSessionPersistenceError::MissingCreatedIdentity,
                "persist the created assessment session before persisting later commands",
            ),
            (
                AssessmentSessionPersistenceError::SequenceConflict,
                "session command sequence was reused by a different command identity",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn session_state_names_cover_the_lifecycle_vocabulary() {
        assert_eq!(SessionState::Created.persist_name(), "created");
        assert_eq!(SessionState::Active.persist_name(), "active");
        assert_eq!(SessionState::Paused.persist_name(), "paused");
        assert_eq!(SessionState::Completed.persist_name(), "completed");
        assert_eq!(SessionState::Scoring.persist_name(), "scoring");
        assert_eq!(SessionState::Scored.persist_name(), "scored");
        assert_eq!(SessionState::Released.persist_name(), "released");
        assert_eq!(SessionState::Expired.persist_name(), "expired");
        assert_eq!(SessionState::Cancelled.persist_name(), "cancelled");
        assert_eq!(SessionState::Invalidated.persist_name(), "invalidated");
        assert_eq!(SessionCommand::Activate.persist_name(), "activate");
        assert_eq!(SessionCommand::BeginScoring.persist_name(), "begin_scoring");
    }

    #[test]
    fn stale_shorter_command_history_is_conflicting_replay() {
        assert!(matches!(
            reject_stale_command_prefix(2, 1),
            Err(AssessmentSessionPersistenceError::ConflictingReplay)
        ));
        assert!(reject_stale_command_prefix(2, 2).is_ok());
        assert!(reject_stale_command_prefix(1, 2).is_ok());
        assert!(reject_stale_command_prefix(0, 0).is_ok());
        assert!(matches!(
            reject_stale_command_prefix(-1, 0),
            Err(AssessmentSessionPersistenceError::ValueOutOfRange)
        ));
    }

    #[test]
    fn start_errors_tell_the_buyer_the_next_action() {
        let unavailable = AssessmentSessionStartError::Creation(
            SessionCreationError::InstrumentReleaseUnavailable,
        );
        assert_eq!(
            unavailable.to_string(),
            "assessment session requires an instrument release currently published for new sessions"
        );
        assert!(std::error::Error::source(&unavailable).is_some());
        let locale = AssessmentSessionStartError::from(SessionCreationError::LocaleMismatch);
        assert_eq!(
            locale.to_string(),
            "assessment session locale must exactly match the published instrument release locale"
        );
        let persist =
            AssessmentSessionStartError::from(AssessmentSessionPersistenceError::ConflictingReplay);
        assert_eq!(
            persist.to_string(),
            "assessment session identity was replayed with conflicting evidence"
        );
        assert!(std::error::Error::source(&persist).is_some());
    }

    #[test]
    fn stored_start_identity_rejects_blank_refs_and_rebinding() {
        const DIGEST: &str =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let stored = AssessmentSession::from_persisted_created(
            "ses_start_identity_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        let release = published_release_for_start_tests(DIGEST);
        assert!(stored_start_identity_matches(
            &stored,
            "ses_start_identity_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &release,
            "ko-KR",
            20_000,
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            " ",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &release,
            "ko-KR",
            20_000,
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            "ses_start_identity_alpha",
            "12",
            &release,
            "ko-KR",
            20_000,
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            "ses_start_identity_alpha",
            "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &release,
            "ko-KR",
            20_000,
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            "ses_start_identity_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &release,
            "en-US",
            20_000,
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            "ses_start_identity_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &release,
            "ko-KR",
            21_000,
        ));
    }

    fn published_release_for_start_tests(digest: &str) -> InstrumentRelease {
        let manifest = InstrumentReleaseManifest::new(
            "release_big_five_ko_v1",
            "instrument_big_five",
            "instrument_version_big_five_ko_v1",
            "construct_big_five",
            &["item_version_001", "item_version_002"],
            "ko-KR",
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_ko_v1",
            Some("norm_version_big_five_ko_v1"),
            "narrative_version_big_five_v1",
            &["consent_service_v1"],
            "intended_use_self_reflection_v1",
            "limitations_nonclinical_v1",
            digest,
        )
        .unwrap();
        let evidence = PublicationEvidenceRecord::new(
            "publication_evidence_big_five_ko_v1",
            "evidence_policy_self_reflection_v1",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            &["item_version_001", "item_version_002"],
            digest,
            "ko-KR",
            "intended_use_self_reflection_v1",
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_ko_v1",
            Some("norm_version_big_five_ko_v1"),
            "limitations_nonclinical_v1",
            PublicationEvidenceProvenance::new(
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                "population_general_adult_v1",
                "administration_web_self_report_v1",
                "measurement_model_big_five_v1",
                10_050,
                None,
            )
            .unwrap(),
            &["rights_ipip_big_five_v1"],
            &["recovery_big_five_ko_v1"],
            &["approval_psychometrics_big_five_ko_v1"],
            PublicationEvidenceStatus::Approved,
        )
        .unwrap();
        let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
        release
            .apply_command(
                "publication_review_start_identity",
                PublicationCommand::SubmitReview,
                10_100,
            )
            .unwrap();
        release.bind_publication_evidence(evidence).unwrap();
        release
            .apply_command(
                "publication_publish_start_identity",
                PublicationCommand::Publish,
                10_200,
            )
            .unwrap();
        release
    }

    #[test]
    fn database_error_wrap_is_instantiated_in_the_library() {
        let Err(source) = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
        else {
            panic!("missing local socket must fail closed")
        };
        let error = AssessmentSessionPersistenceError::from(source);
        assert_eq!(
            error.to_string(),
            "PostgreSQL assessment-session persistence failed"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
}
