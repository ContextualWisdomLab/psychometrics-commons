//! `PostgreSQL` 18 persistence for assessment-session identity and command history.
//!
//! This module stores the participant, published-release, version, content-digest,
//! and locale identity copied at session creation. It does not rewrite provenance
//! when the release is later suspended or retired. New sessions must start
//! through [`created_session_for_start`], [`start_created_assessment_session`],
//! [`created_session_for_start_from_published_snapshot`], or
//! [`start_created_assessment_session_from_stored_release`]. Start locks the
//! stored `instrument_release` row in the same transaction so a stale in-memory
//! Published object cannot insert after persist Suspend or Retire. Created
//! identity is inserted only for
//! [`SessionState::Created`]. Later lifecycle states persist as append-only
//! command history plus a current-state projection. A shorter persist than
//! already stored fails closed so a stale worker cannot rewind that projection.
//! Command persist locks the created-session header row before inserting or
//! counting commands. Load restores created identity without re-checking
//! publication eligibility, then replays stored commands. Replay requires
//! `READ COMMITTED`.

use crate::instrument::InstrumentRelease;
use crate::postgres_instrument_release::{
    load_published_instrument_release, InstrumentReleaseQueryError,
    PublishedInstrumentReleaseSnapshot,
};
use crate::reference::normalized_reference;
use crate::session::{
    AcceptedSessionCommand, AssessmentSession, SessionCommand, SessionCreationError, SessionState,
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

/// Fail-closed error for starting a created session from a live published release.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssessmentSessionStartError {
    /// A session or participant reference was blank or numeric-like.
    InvalidReference,
    /// The server-authoritative session creation timestamp was zero.
    InvalidTimestamp,
    /// The selected immutable release is not currently allowed to begin new sessions.
    InstrumentReleaseUnavailable,
    /// The requested assessment locale does not exactly match the published release locale.
    LocaleMismatch,
    /// Stored publication evidence could not be used as a start source.
    InvalidStoredRelease,
    /// The created session could not be persisted.
    Persistence(AssessmentSessionPersistenceError),
}

impl Display for AssessmentSessionStartError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "use opaque non-numeric session and participant references to start a session"
            }
            Self::InvalidTimestamp => {
                "use a server creation time greater than zero to start a session"
            }
            Self::InstrumentReleaseUnavailable => {
                "publish the exact instrument release before starting a new session"
            }
            Self::LocaleMismatch => "start the session with the exact published release locale",
            Self::InvalidStoredRelease => {
                "repair the stored instrument release before starting a new session"
            }
            Self::Persistence(_) => {
                "session start could not persist the created session; retry the exact start or repair the store"
            }
        })
    }
}

impl Error for AssessmentSessionStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Persistence(error) => Some(error),
            Self::InvalidReference
            | Self::InvalidTimestamp
            | Self::InstrumentReleaseUnavailable
            | Self::LocaleMismatch
            | Self::InvalidStoredRelease => None,
        }
    }
}

impl From<SessionCreationError> for AssessmentSessionStartError {
    fn from(error: SessionCreationError) -> Self {
        match error {
            SessionCreationError::InvalidReference => Self::InvalidReference,
            SessionCreationError::InvalidTimestamp => Self::InvalidTimestamp,
            SessionCreationError::InstrumentReleaseUnavailable => {
                Self::InstrumentReleaseUnavailable
            }
            SessionCreationError::LocaleMismatch => Self::LocaleMismatch,
        }
    }
}

impl From<AssessmentSessionPersistenceError> for AssessmentSessionStartError {
    fn from(error: AssessmentSessionPersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<InstrumentReleaseQueryError> for AssessmentSessionStartError {
    fn from(error: InstrumentReleaseQueryError) -> Self {
        match error {
            InstrumentReleaseQueryError::InvalidReference => Self::InvalidReference,
            InstrumentReleaseQueryError::InvalidLocale
            | InstrumentReleaseQueryError::LocaleMismatch => Self::LocaleMismatch,
            InstrumentReleaseQueryError::NotFound | InstrumentReleaseQueryError::NotPublished => {
                Self::InstrumentReleaseUnavailable
            }
            InstrumentReleaseQueryError::InvalidStoredValue => Self::InvalidStoredRelease,
            InstrumentReleaseQueryError::Database(error) => {
                Self::Persistence(AssessmentSessionPersistenceError::from(error))
            }
        }
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

/// Build a created session that is legal to persist as a new start.
///
/// HTTP `POST /v1/sessions` and any other start path must call this,
/// [`start_created_assessment_session`],
/// [`created_session_for_start_from_published_snapshot`], or
/// [`start_created_assessment_session_from_stored_release`], rather than
/// [`AssessmentSession::from_persisted_created`]. This uses
/// [`AssessmentSession::new`], so a suspended or retired in-memory release
/// cannot begin a new session. Durable start must still lock the stored
/// publication row.
///
/// # Errors
///
/// Returns [`AssessmentSessionStartError`] when the session or participant
/// reference is invalid, the timestamp is zero, the release does not currently
/// accept new sessions, or the requested locale is not the release locale.
pub fn created_session_for_start(
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<AssessmentSession, AssessmentSessionStartError> {
    AssessmentSession::new(
        session_ref,
        participant_ref,
        release,
        requested_locale,
        created_at_unix_ms,
    )
    .map_err(AssessmentSessionStartError::from)
}

/// Start one created session from a live published release and persist it.
///
/// This is the in-memory start boundary: it calls [`created_session_for_start`],
/// locks the stored `instrument_release` row, and then
/// [`persist_assessment_session`]. A stale Published object fails when the
/// stored row is missing, unpublished, or digest-mismatched. Exact replay of
/// the same start is idempotent. It does not treat load as authorization and
/// does not accept a reconstituted session.
///
/// # Errors
///
/// Returns [`AssessmentSessionStartError`] for an unpublished or mismatched
/// release, invalid start identity, or a persistence failure.
pub fn start_created_assessment_session(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    participant_ref: &str,
    release: &InstrumentRelease,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<(AssessmentSession, AssessmentSessionPersistenceDisposition), AssessmentSessionStartError>
{
    let session = created_session_for_start(
        session_ref,
        participant_ref,
        release,
        requested_locale,
        created_at_unix_ms,
    )?;
    require_locked_published_release(transaction, &session, requested_locale)?;
    let disposition = persist_assessment_session(transaction, &session)?;
    Ok((session, disposition))
}

/// Build a created session from a store-validated published-release snapshot.
///
/// HTTP `POST /v1/sessions` must call this after
/// [`load_published_instrument_release`], or call
/// [`start_created_assessment_session_from_stored_release`], so a stale
/// in-memory Published object cannot start a session after the stored release
/// is suspended or retired.
///
/// # Errors
///
/// Returns [`AssessmentSessionStartError`] when the session or participant
/// reference is invalid, the timestamp is zero, or the requested locale is not
/// the snapshot locale.
pub fn created_session_for_start_from_published_snapshot(
    session_ref: &str,
    participant_ref: &str,
    snapshot: &PublishedInstrumentReleaseSnapshot,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<AssessmentSession, AssessmentSessionStartError> {
    AssessmentSession::from_currently_published_manifest(
        session_ref,
        participant_ref,
        snapshot.manifest(),
        requested_locale,
        created_at_unix_ms,
    )
    .map_err(AssessmentSessionStartError::from)
}

/// Start one created session from the stored published release and persist it.
///
/// This locks the exact release and locale from the same transaction, then
/// calls [`created_session_for_start_from_published_snapshot`] and
/// [`persist_assessment_session`]. A stored Draft, Review, Suspended, or
/// Retired release fails before insert.
///
/// # Errors
///
/// Returns [`AssessmentSessionStartError`] when the stored release is missing,
/// unpublished, locale-mismatched, corrupt, or persist fails.
pub fn start_created_assessment_session_from_stored_release(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    participant_ref: &str,
    instrument_release_ref: &str,
    requested_locale: &str,
    created_at_unix_ms: u64,
) -> Result<(AssessmentSession, AssessmentSessionPersistenceDisposition), AssessmentSessionStartError>
{
    let snapshot =
        load_published_instrument_release(transaction, instrument_release_ref, requested_locale)?;
    let session = created_session_for_start_from_published_snapshot(
        session_ref,
        participant_ref,
        &snapshot,
        requested_locale,
        created_at_unix_ms,
    )?;
    let disposition = persist_assessment_session(transaction, &session)?;
    Ok((session, disposition))
}

fn require_locked_published_release(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
    requested_locale: &str,
) -> Result<(), AssessmentSessionStartError> {
    let snapshot = load_published_instrument_release(
        transaction,
        session.instrument_release_ref(),
        requested_locale,
    )?;
    if snapshot.manifest().content_digest() != session.instrument_release_content_digest()
        || snapshot.manifest().instrument_version_ref() != session.instrument_version_ref()
        || snapshot.manifest().locale() != session.locale()
    {
        return Err(AssessmentSessionStartError::InvalidStoredRelease);
    }
    Ok(())
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

/// Persist accepted command history and the current lifecycle-state projection.
///
/// The created-session identity row must already exist. Exact replay of the same
/// command reference, sequence, command, and resulting state is idempotent.
/// Rebinding command evidence, reusing a sequence under another command
/// identity, or persisting a shorter history than already stored fails closed so
/// a stale Activate-only worker cannot rewind Pause/Resume. The created-session
/// header row is locked with `SELECT … FOR UPDATE` before commands are inserted
/// or counted, so a concurrent Activate-only persist cannot count a prefix and
/// then overwrite a later Pause/Resume projection. Load later reconstitutes
/// created identity and replays these commands.
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
        reject_stale_command_prefix, AssessmentSessionPersistenceError, AssessmentSessionStartError,
    };
    use crate::postgres_instrument_release::InstrumentReleaseQueryError;
    use crate::session::{SessionCommand, SessionCreationError, SessionState};

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

    #[test]
    fn session_start_errors_map_creation_failures_and_keep_persistence_source() {
        for (creation, expected) in [
            (
                SessionCreationError::InvalidReference,
                AssessmentSessionStartError::InvalidReference,
            ),
            (
                SessionCreationError::InvalidTimestamp,
                AssessmentSessionStartError::InvalidTimestamp,
            ),
            (
                SessionCreationError::InstrumentReleaseUnavailable,
                AssessmentSessionStartError::InstrumentReleaseUnavailable,
            ),
            (
                SessionCreationError::LocaleMismatch,
                AssessmentSessionStartError::LocaleMismatch,
            ),
        ] {
            let mapped = AssessmentSessionStartError::from(creation);
            assert_eq!(mapped.to_string(), expected.to_string());
            assert!(std::error::Error::source(&mapped).is_none());
        }

        let persistence =
            AssessmentSessionStartError::from(AssessmentSessionPersistenceError::ConflictingReplay);
        assert!(matches!(
            persistence,
            AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::ConflictingReplay
            )
        ));
        assert!(std::error::Error::source(&persistence).is_some());

        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::NotPublished)
                .to_string(),
            AssessmentSessionStartError::InstrumentReleaseUnavailable.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::NotFound).to_string(),
            AssessmentSessionStartError::InstrumentReleaseUnavailable.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::InvalidLocale)
                .to_string(),
            AssessmentSessionStartError::LocaleMismatch.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::LocaleMismatch)
                .to_string(),
            AssessmentSessionStartError::LocaleMismatch.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::InvalidReference)
                .to_string(),
            AssessmentSessionStartError::InvalidReference.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::from(InstrumentReleaseQueryError::InvalidStoredValue)
                .to_string(),
            AssessmentSessionStartError::InvalidStoredRelease.to_string()
        );
        assert_eq!(
            AssessmentSessionStartError::InvalidStoredRelease.to_string(),
            "repair the stored instrument release before starting a new session"
        );
        assert!(
            std::error::Error::source(&AssessmentSessionStartError::InvalidStoredRelease).is_none()
        );

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
        let mapped = AssessmentSessionStartError::from(InstrumentReleaseQueryError::from(source));
        assert!(matches!(
            mapped,
            AssessmentSessionStartError::Persistence(AssessmentSessionPersistenceError::Database(
                _
            ))
        ));
        assert!(std::error::Error::source(&mapped).is_some());
    }
}
