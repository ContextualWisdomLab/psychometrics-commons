//! `PostgreSQL` 18 persistence for assessment-session identity and command history.
//!
//! This module stores the participant, published-release, version, content-digest,
//! and locale identity copied at session creation. It does not rewrite provenance
//! when the release is later suspended or retired. New sessions must start
//! through [`created_session_for_start`], [`start_created_assessment_session`],
//! [`created_session_for_start_from_published_snapshot`], or
//! [`start_created_assessment_session_from_stored_release`]. Start locks the
//! stored `instrument_release` row in the same transaction so a stale in-memory
//! Published object cannot insert after persist Suspend or Retire. First insert
//! through [`persist_assessment_session`] takes the same lock, so a reconstituted
//! Created aggregate cannot insert after that later persist. When that lock
//! finds a missing or unpublished release, persist still classifies an exact
//! stored Created row as duplicate so a concurrent retry after the first insert
//! commits cannot turn a later Suspend or Retire into a false unpublished
//! failure. Exact replay of an already stored start still returns the original
//! session after a later persist Suspend or Retire, so a buyer who already
//! started can retry. Created
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
    /// First insert requires a currently published stored release locked in this transaction.
    UnpublishedStart,
    /// Stored publication evidence does not match the created-session identity.
    InvalidStartRelease,
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
            Self::UnpublishedStart => {
                "start the session from a currently published stored release; do not persist a reconstituted Created row after suspend or retire"
            }
            Self::InvalidStartRelease => {
                "repair the stored instrument release before persisting a new session"
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

impl From<InstrumentReleaseQueryError> for AssessmentSessionPersistenceError {
    fn from(error: InstrumentReleaseQueryError) -> Self {
        match error {
            InstrumentReleaseQueryError::NotFound | InstrumentReleaseQueryError::NotPublished => {
                Self::UnpublishedStart
            }
            InstrumentReleaseQueryError::InvalidLocale
            | InstrumentReleaseQueryError::LocaleMismatch
            | InstrumentReleaseQueryError::InvalidStoredValue
            | InstrumentReleaseQueryError::InvalidReference => Self::InvalidStartRelease,
            InstrumentReleaseQueryError::Database(error) => Self::Database(error),
        }
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
/// stored row is missing, unpublished, or digest-mismatched and no exact stored
/// start exists. Exact replay of an already stored start returns the original
/// session after a later persist Suspend or Retire. It does not treat load as
/// authorization and does not accept a reconstituted first insert.
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
    match created_session_for_start(
        session_ref,
        participant_ref,
        release,
        requested_locale,
        created_at_unix_ms,
    ) {
        Ok(session) => {
            match require_locked_published_release(transaction, &session, requested_locale) {
                Ok(()) => {
                    let disposition = persist_assessment_session(transaction, &session)?;
                    Ok((session, disposition))
                }
                Err(AssessmentSessionStartError::InstrumentReleaseUnavailable) => {
                    replay_started_session_after_publication_block(
                        transaction,
                        &StartedSessionReplayRequest {
                            session_ref,
                            participant_ref,
                            instrument_release_ref: session.instrument_release_ref(),
                            instrument_version_ref: Some(session.instrument_version_ref()),
                            content_digest: Some(session.instrument_release_content_digest()),
                            requested_locale,
                            created_at_unix_ms,
                        },
                    )
                }
                Err(error) => Err(error),
            }
        }
        Err(AssessmentSessionStartError::InstrumentReleaseUnavailable) => {
            match load_published_instrument_release(
                transaction,
                release.manifest().release_ref(),
                requested_locale,
            ) {
                Ok(_)
                | Err(
                    InstrumentReleaseQueryError::NotPublished
                    | InstrumentReleaseQueryError::NotFound,
                ) => replay_started_session_after_publication_block(
                    transaction,
                    &StartedSessionReplayRequest {
                        session_ref,
                        participant_ref,
                        instrument_release_ref: release.manifest().release_ref(),
                        instrument_version_ref: Some(release.manifest().instrument_version_ref()),
                        content_digest: Some(release.manifest().content_digest()),
                        requested_locale,
                        created_at_unix_ms,
                    },
                ),
                Err(error) => Err(AssessmentSessionStartError::from(error)),
            }
        }
        Err(error) => Err(error),
    }
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
/// Retired release fails before a first insert. Exact replay of an already
/// stored start still returns the original session after that later persist
/// Suspend or Retire.
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
    match load_published_instrument_release(transaction, instrument_release_ref, requested_locale) {
        Ok(snapshot) => {
            #[allow(clippy::question_mark)]
            let session = match created_session_for_start_from_published_snapshot(
                session_ref,
                participant_ref,
                &snapshot,
                requested_locale,
                created_at_unix_ms,
            ) {
                Ok(session) => session,
                Err(error) => return Err(error),
            };
            let disposition = persist_assessment_session(transaction, &session)?;
            Ok((session, disposition))
        }
        Err(InstrumentReleaseQueryError::NotPublished | InstrumentReleaseQueryError::NotFound) => {
            replay_started_session_after_publication_block(
                transaction,
                &StartedSessionReplayRequest {
                    session_ref,
                    participant_ref,
                    instrument_release_ref,
                    instrument_version_ref: None,
                    content_digest: None,
                    requested_locale,
                    created_at_unix_ms,
                },
            )
        }
        Err(error) => Err(AssessmentSessionStartError::from(error)),
    }
}

/// Identity a later start retry must match after publication no longer accepts new sessions.
struct StartedSessionReplayRequest<'a> {
    session_ref: &'a str,
    participant_ref: &'a str,
    instrument_release_ref: &'a str,
    instrument_version_ref: Option<&'a str>,
    content_digest: Option<&'a str>,
    requested_locale: &'a str,
    created_at_unix_ms: u64,
}

/// Return an exact stored start after the release no longer accepts new sessions.
fn replay_started_session_after_publication_block(
    transaction: &mut Transaction<'_>,
    request: &StartedSessionReplayRequest<'_>,
) -> Result<(AssessmentSession, AssessmentSessionPersistenceDisposition), AssessmentSessionStartError>
{
    let Some(stored) = load_assessment_session(transaction, request.session_ref)? else {
        return Err(AssessmentSessionStartError::InstrumentReleaseUnavailable);
    };
    if stored_start_identity_matches(&stored, request) {
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
    request: &StartedSessionReplayRequest<'_>,
) -> bool {
    let Some(session_ref) = normalized_reference(request.session_ref) else {
        return false;
    };
    let Some(participant_ref) = normalized_reference(request.participant_ref) else {
        return false;
    };
    let Some(instrument_release_ref) = normalized_reference(request.instrument_release_ref) else {
        return false;
    };
    stored.session_ref() == session_ref
        && stored.participant_ref() == participant_ref
        && stored.instrument_release_ref() == instrument_release_ref
        && request
            .instrument_version_ref
            .is_none_or(|version_ref| stored.instrument_version_ref() == version_ref)
        && request
            .content_digest
            .is_none_or(|digest| stored.instrument_release_content_digest() == digest)
        && stored.locale() == request.requested_locale
        && stored.created_at_unix_ms() == request.created_at_unix_ms
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
    if !published_snapshot_matches_session(&snapshot, session) {
        return Err(AssessmentSessionStartError::InvalidStoredRelease);
    }
    Ok(())
}

/// Lock stored publication evidence before the first created-session insert.
fn require_published_release_for_first_insert(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
) -> Result<(), AssessmentSessionPersistenceError> {
    let snapshot = load_published_instrument_release(
        transaction,
        session.instrument_release_ref(),
        session.locale(),
    )?;
    if published_snapshot_matches_session(&snapshot, session) {
        Ok(())
    } else {
        Err(AssessmentSessionPersistenceError::InvalidStartRelease)
    }
}

/// Compare locked publication evidence with the created-session identity.
fn published_snapshot_matches_session(
    snapshot: &PublishedInstrumentReleaseSnapshot,
    session: &AssessmentSession,
) -> bool {
    snapshot.manifest().content_digest() == session.instrument_release_content_digest()
        && snapshot.manifest().instrument_version_ref() == session.instrument_version_ref()
        && snapshot.manifest().locale() == session.locale()
}

/// Exact persist replay stays legal after the first-insert publication seal fails.
fn first_insert_seal_allows_exact_replay(error: &AssessmentSessionPersistenceError) -> bool {
    match error {
        AssessmentSessionPersistenceError::UnpublishedStart
        | AssessmentSessionPersistenceError::InvalidStartRelease => true,
        AssessmentSessionPersistenceError::ValueOutOfRange
        | AssessmentSessionPersistenceError::UnsupportedInitialState
        | AssessmentSessionPersistenceError::ConflictingReplay
        | AssessmentSessionPersistenceError::UnsupportedIsolationLevel
        | AssessmentSessionPersistenceError::InvalidReference
        | AssessmentSessionPersistenceError::InvalidStoredIdentity
        | AssessmentSessionPersistenceError::UnsupportedStoredState
        | AssessmentSessionPersistenceError::MissingCreatedIdentity
        | AssessmentSessionPersistenceError::SequenceConflict
        | AssessmentSessionPersistenceError::Database(_) => false,
    }
}

/// Classify an exact stored Created row after the publication seal fails closed.
fn replay_existing_created_session_after_seal(
    transaction: &mut Transaction<'_>,
    session: &AssessmentSession,
    session_ref: &str,
    created_at_unix_ms: i64,
    seal_error: AssessmentSessionPersistenceError,
) -> Result<AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError> {
    let existing = match transaction.query_opt(
        "SELECT 1 FROM assessment_session WHERE session_ref = $1 FOR UPDATE",
        &[&session_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
    if existing.is_some() {
        classify_existing_session(transaction, session, session_ref, created_at_unix_ms)
    } else {
        Err(seal_error)
    }
}

/// Persist one created assessment-session identity bound to a published release.
///
/// A first insert locks the stored `instrument_release` row and fails closed
/// when that row is missing, unpublished, or digest/version/locale-mismatched
/// and no exact stored Created row exists. Exact replay of an already stored
/// Created row stays legal after a later persist Suspend or Retire, including
/// when a concurrent first insert committed while this transaction waited on
/// the release lock. Rebinding any stored field fails closed.
/// [`AssessmentSession::new`] validates and normalizes session and participant
/// references. This function stores those references without validating them
/// again.
///
/// # Errors
///
/// Returns [`AssessmentSessionPersistenceError`] for unsupported isolation,
/// a non-created session, an unpublished first insert, conflicting replay,
/// an out-of-range timestamp, or a database failure.
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
    match require_published_release_for_first_insert(transaction, session) {
        Ok(()) => {}
        Err(error) if first_insert_seal_allows_exact_replay(&error) => {
            return replay_existing_created_session_after_seal(
                transaction,
                session,
                session_ref,
                created_at_unix_ms,
                error,
            );
        }
        Err(error) => return Err(error),
    }
    let inserted = match transaction.execute(
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
    ) {
        Ok(count) => count,
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
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
    let stored_command_count: i64 = match transaction.query_one(
        "SELECT COUNT(*) FROM assessment_session_command WHERE session_ref = $1",
        &[&session_ref],
    ) {
        Ok(row) => row.get(0),
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
    reject_stale_command_prefix(stored_command_count, session.accepted_commands().len())?;
    match transaction.execute(
        "UPDATE assessment_session SET session_state = $2 WHERE session_ref = $1",
        &[&session_ref, &session.state().persist_name()],
    ) {
        Ok(_) => {}
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    }
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
    let commands = match transaction.query(
        "SELECT command_ref, command_sequence, command_name, resulting_state
         FROM assessment_session_command
         WHERE session_ref = $1
         ORDER BY command_sequence",
        &[&session_ref],
    ) {
        Ok(rows) => rows,
        Err(error) => return Err(AssessmentSessionPersistenceError::from(error)),
    };
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
fn load_command_replay_row(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    command_ref: &str,
) -> Result<postgres::Row, AssessmentSessionPersistenceError> {
    match transaction.query_one(
        "SELECT command_sequence, command_name, resulting_state
         FROM assessment_session_command
         WHERE session_ref = $1 AND command_ref = $2",
        &[&session_ref, &command_ref],
    ) {
        Ok(row) => Ok(row),
        Err(error) => Err(AssessmentSessionPersistenceError::from(error)),
    }
}

fn lookup_command_ref_at_sequence(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    command_sequence: i64,
) -> Result<Option<String>, AssessmentSessionPersistenceError> {
    match transaction.query_opt(
        "SELECT command_ref FROM assessment_session_command
         WHERE session_ref = $1 AND command_sequence = $2",
        &[&session_ref, &command_sequence],
    ) {
        Ok(row) => Ok(row.map(|row| row.get(0))),
        Err(error) => Err(AssessmentSessionPersistenceError::from(error)),
    }
}

fn insert_session_command(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    command: &AcceptedSessionCommand,
    command_sequence: i64,
) -> Result<u64, AssessmentSessionPersistenceError> {
    match transaction.execute(
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
    ) {
        Ok(count) => Ok(count),
        Err(error) => Err(AssessmentSessionPersistenceError::from(error)),
    }
}

fn classify_command_replay(
    row: &postgres::Row,
    command: &AcceptedSessionCommand,
    command_sequence: i64,
) -> Result<bool, AssessmentSessionPersistenceError> {
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

fn persist_one_session_command(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    command: &AcceptedSessionCommand,
) -> Result<bool, AssessmentSessionPersistenceError> {
    let command_sequence = postgres_bigint(command.sequence())?;
    if let Some(stored_ref) =
        lookup_command_ref_at_sequence(transaction, session_ref, command_sequence)?
    {
        if stored_ref != command.command_ref() {
            return Err(AssessmentSessionPersistenceError::SequenceConflict);
        }
    }
    let inserted = insert_session_command(transaction, session_ref, command, command_sequence)?;
    if inserted == 1 {
        return Ok(true);
    }
    let row = load_command_replay_row(transaction, session_ref, command.command_ref())?;
    classify_command_replay(&row, command, command_sequence)
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
        apply_assessment_session_migration, created_session_for_start_from_published_snapshot,
        first_insert_seal_allows_exact_replay, load_assessment_session, persist_assessment_session,
        persist_assessment_session_commands, published_snapshot_matches_session,
        reject_stale_command_prefix, start_created_assessment_session,
        start_created_assessment_session_from_stored_release, stored_start_identity_matches,
        AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError,
        AssessmentSessionStartError, StartedSessionReplayRequest,
    };
    use crate::instrument::{
        InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
        PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    };
    use crate::postgres_instrument_release::{
        apply_instrument_release_migration, persist_instrument_release,
        InstrumentReleaseQueryError, PublishedInstrumentReleaseSnapshot,
    };
    use crate::session::{AssessmentSession, SessionCommand, SessionCreationError, SessionState};
    use postgres::{Client, NoTls};

    const REPLAY_DIGEST: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn published_snapshot_for_session_match() -> PublishedInstrumentReleaseSnapshot {
        PublishedInstrumentReleaseSnapshot::from_published_manifest(
            InstrumentReleaseManifest::new(
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
                REPLAY_DIGEST,
            )
            .unwrap(),
            10_000,
        )
        .unwrap()
    }

    #[test]
    fn published_snapshot_must_match_session_digest_version_and_locale() {
        let snapshot = published_snapshot_for_session_match();
        let matching = AssessmentSession::from_persisted_created(
            "ses_snapshot_match_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert!(published_snapshot_matches_session(&snapshot, &matching));
        let locale_drift = AssessmentSession::from_persisted_created(
            "ses_snapshot_match_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "en-US",
            20_000,
        )
        .unwrap();
        assert!(!published_snapshot_matches_session(
            &snapshot,
            &locale_drift
        ));
        let version_drift = AssessmentSession::from_persisted_created(
            "ses_snapshot_match_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_en_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert!(!published_snapshot_matches_session(
            &snapshot,
            &version_drift
        ));
        let digest_drift = AssessmentSession::from_persisted_created(
            "ses_snapshot_match_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert!(!published_snapshot_matches_session(
            &snapshot,
            &digest_drift
        ));
    }

    fn stored_start_replay_session() -> AssessmentSession {
        AssessmentSession::from_persisted_created(
            "ses_start_replay_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap()
    }

    fn start_replay_request<'a>(
        session_ref: &'a str,
        participant_ref: &'a str,
        instrument_release_ref: &'a str,
        instrument_version_ref: Option<&'a str>,
        content_digest: Option<&'a str>,
        requested_locale: &'a str,
        created_at_unix_ms: u64,
    ) -> StartedSessionReplayRequest<'a> {
        StartedSessionReplayRequest {
            session_ref,
            participant_ref,
            instrument_release_ref,
            instrument_version_ref,
            content_digest,
            requested_locale,
            created_at_unix_ms,
        }
    }

    #[test]
    fn exact_start_identity_matches_stored_session_and_rejects_rebind() {
        let stored = stored_start_replay_session();
        assert!(stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                Some("instrument_version_big_five_ko_v1"),
                Some(REPLAY_DIGEST),
                "ko-KR",
                20_000,
            ),
        ));
        assert!(stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                Some("instrument_version_rebinding_v2"),
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "12345",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
    }

    #[test]
    fn exact_start_identity_rejects_numeric_refs_and_release_locale_clock_rebinds() {
        let stored = stored_start_replay_session();
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "12",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "12",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_rebinding",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_en_v1",
                None,
                None,
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                Some("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
                "ko-KR",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                None,
                "en-US",
                20_000,
            ),
        ));
        assert!(!stored_start_identity_matches(
            &stored,
            &start_replay_request(
                "ses_start_replay_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                None,
                None,
                "ko-KR",
                20_001,
            ),
        ));
    }

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
            (
                AssessmentSessionPersistenceError::UnpublishedStart,
                "start the session from a currently published stored release; do not persist a reconstituted Created row after suspend or retire",
            ),
            (
                AssessmentSessionPersistenceError::InvalidStartRelease,
                "repair the stored instrument release before persisting a new session",
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
        let source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .map(|_| ())
            .expect_err("missing local socket must fail closed");
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

        let source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .map(|_| ())
            .expect_err("missing local socket must fail closed");
        let mapped = AssessmentSessionStartError::from(InstrumentReleaseQueryError::from(source));
        assert!(matches!(
            mapped,
            AssessmentSessionStartError::Persistence(AssessmentSessionPersistenceError::Database(
                _
            ))
        ));
        assert!(std::error::Error::source(&mapped).is_some());
    }

    #[test]
    fn persist_maps_unpublished_stored_release_to_first_insert_seal() {
        assert!(matches!(
            AssessmentSessionPersistenceError::from(InstrumentReleaseQueryError::NotPublished),
            AssessmentSessionPersistenceError::UnpublishedStart
        ));
        assert!(matches!(
            AssessmentSessionPersistenceError::from(InstrumentReleaseQueryError::NotFound),
            AssessmentSessionPersistenceError::UnpublishedStart
        ));
        for error in [
            InstrumentReleaseQueryError::InvalidLocale,
            InstrumentReleaseQueryError::LocaleMismatch,
            InstrumentReleaseQueryError::InvalidStoredValue,
            InstrumentReleaseQueryError::InvalidReference,
        ] {
            assert!(matches!(
                AssessmentSessionPersistenceError::from(error),
                AssessmentSessionPersistenceError::InvalidStartRelease
            ));
        }
        assert_eq!(
            AssessmentSessionPersistenceError::UnpublishedStart.to_string(),
            "start the session from a currently published stored release; do not persist a reconstituted Created row after suspend or retire"
        );
        assert_eq!(
            AssessmentSessionPersistenceError::InvalidStartRelease.to_string(),
            "repair the stored instrument release before persisting a new session"
        );
        assert!(
            std::error::Error::source(&AssessmentSessionPersistenceError::UnpublishedStart)
                .is_none()
        );
        assert!(
            std::error::Error::source(&AssessmentSessionPersistenceError::InvalidStartRelease)
                .is_none()
        );

        let persistence =
            AssessmentSessionStartError::from(AssessmentSessionPersistenceError::UnpublishedStart);
        assert!(matches!(
            persistence,
            AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::UnpublishedStart
            )
        ));
        assert_eq!(
            persistence.to_string(),
            "session start could not persist the created session; retry the exact start or repair the store"
        );
    }

    #[test]
    fn first_insert_seal_replays_only_publication_boundary_errors() {
        assert!(first_insert_seal_allows_exact_replay(
            &AssessmentSessionPersistenceError::UnpublishedStart
        ));
        assert!(first_insert_seal_allows_exact_replay(
            &AssessmentSessionPersistenceError::InvalidStartRelease
        ));
        for error in [
            AssessmentSessionPersistenceError::ValueOutOfRange,
            AssessmentSessionPersistenceError::UnsupportedInitialState,
            AssessmentSessionPersistenceError::ConflictingReplay,
            AssessmentSessionPersistenceError::UnsupportedIsolationLevel,
            AssessmentSessionPersistenceError::InvalidReference,
            AssessmentSessionPersistenceError::InvalidStoredIdentity,
            AssessmentSessionPersistenceError::UnsupportedStoredState,
            AssessmentSessionPersistenceError::MissingCreatedIdentity,
            AssessmentSessionPersistenceError::SequenceConflict,
        ] {
            assert!(
                !first_insert_seal_allows_exact_replay(&error),
                "non-publication persist errors must not be rewritten as exact replay: {error}"
            );
        }
        let source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .map(|_| ())
            .expect_err("missing local socket must fail closed");
        let database = AssessmentSessionPersistenceError::from(source);
        assert!(!first_insert_seal_allows_exact_replay(&database));
        let query_source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .map(|_| ())
            .expect_err("missing local socket must fail closed");
        assert!(matches!(
            AssessmentSessionPersistenceError::from(InstrumentReleaseQueryError::from(
                query_source
            )),
            AssessmentSessionPersistenceError::Database(_)
        ));
    }

    #[test]
    fn load_and_command_persist_map_missing_relation_to_database_error() {
        use super::{load_assessment_session, persist_assessment_session_commands};
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO assessment_session_load_missing;")
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_load_missing"),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));

        let mut paused = AssessmentSession::from_persisted_created(
            "ses_command_missing",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_missing", 1, SessionCommand::Activate)
            .unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn start_maps_missing_store_and_invalid_identity_from_the_library() {
        use super::start_created_assessment_session_from_stored_release;
        use crate::instrument::{InstrumentRelease, InstrumentReleaseManifest};
        use crate::postgres_assessment_session::start_created_assessment_session;
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO assessment_session_start_missing;")
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "ses_start_missing",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::Database(_)
            ))
        ));

        let draft = InstrumentRelease::new(
            InstrumentReleaseManifest::new(
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
                REPLAY_DIGEST,
            )
            .unwrap(),
            10_000,
        )
        .unwrap();
        assert!(matches!(
            start_created_assessment_session(
                &mut transaction,
                "12",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &draft,
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
        assert!(matches!(
            start_created_assessment_session(
                &mut transaction,
                "ses_unpub_missing",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &draft,
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::Database(_)
            ))
        ));
        transaction.rollback().unwrap();
    }

    fn published_release_for_unpublished_memory_start() -> InstrumentRelease {
        let mut published = InstrumentRelease::new(
            InstrumentReleaseManifest::new(
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
                REPLAY_DIGEST,
            )
            .unwrap(),
            10_000,
        )
        .unwrap();
        published
            .apply_command(
                "publication_review_unpublished_memory",
                PublicationCommand::SubmitReview,
                10_100,
            )
            .unwrap();
        published
            .bind_publication_evidence(
                PublicationEvidenceRecord::new(
                    "publication_evidence_big_five_ko_v1",
                    "evidence_policy_self_reflection_v1",
                    "release_big_five_ko_v1",
                    "instrument_version_big_five_ko_v1",
                    &["item_version_001", "item_version_002"],
                    REPLAY_DIGEST,
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
                .unwrap(),
            )
            .unwrap();
        published
            .apply_command(
                "publication_publish_unpublished_memory",
                PublicationCommand::Publish,
                10_200,
            )
            .unwrap();
        published
    }

    #[test]
    fn unpublished_in_memory_start_replays_exact_stored_session() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_unpublished_memory CASCADE; \
                 CREATE SCHEMA assessment_session_unpublished_memory; \
                 SET search_path TO assessment_session_unpublished_memory;",
            )
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        apply_assessment_session_migration(&mut client).unwrap();

        let published = published_release_for_unpublished_memory_start();
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &published).unwrap();
        let (started, inserted) = start_created_assessment_session(
            &mut transaction,
            "ses_unpublished_memory_replay",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &published,
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(inserted, AssessmentSessionPersistenceDisposition::Inserted);
        assert_eq!(started.session_ref(), "ses_unpublished_memory_replay");
        transaction.commit().unwrap();

        let draft = InstrumentRelease::new(published.manifest().clone(), 10_000).unwrap();
        let mut suspended = published;
        suspended
            .apply_command(
                "publication_suspend_unpublished_memory",
                PublicationCommand::Suspend,
                10_300,
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        let (replayed, disposition) = start_created_assessment_session(
            &mut transaction,
            "ses_unpublished_memory_replay",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &suspended,
            "ko-KR",
            20_000,
        )
        .expect("a buyer who already started must retry after the in-memory catalog is suspended");
        assert_eq!(
            disposition,
            AssessmentSessionPersistenceDisposition::Duplicate
        );
        assert_eq!(replayed.session_ref(), "ses_unpublished_memory_replay");
        assert!(matches!(
            start_created_assessment_session(
                &mut transaction,
                "ses_unpublished_memory_new",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &draft,
                "ko-KR",
                21_000,
            ),
            Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
        ));
        assert!(matches!(
            start_created_assessment_session(
                &mut transaction,
                "ses_unpublished_memory_new",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &suspended,
                "ko-KR",
                21_000,
            ),
            Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn first_insert_seal_replay_maps_missing_session_relation() {
        use super::persist_assessment_session;
        use crate::postgres_instrument_release::apply_instrument_release_migration;
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_seal_missing CASCADE; \
                 CREATE SCHEMA assessment_session_seal_missing; \
                 SET search_path TO assessment_session_seal_missing;",
            )
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        let created = AssessmentSession::from_persisted_created(
            "ses_seal_missing",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session(&mut transaction, &created),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn command_persist_maps_header_update_failure_to_database_error() {
        use super::{apply_assessment_session_migration, persist_assessment_session_commands};
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_update_sink CASCADE; \
                 CREATE SCHEMA assessment_session_update_sink; \
                 SET search_path TO assessment_session_update_sink;",
            )
            .unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_update_sink', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 ); \
                 CREATE FUNCTION assessment_session_reject_update() RETURNS trigger \
                 LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'assessment_session update sink'; END $$; \
                 CREATE TRIGGER assessment_session_reject_update \
                 BEFORE UPDATE ON assessment_session \
                 FOR EACH STATEMENT EXECUTE FUNCTION assessment_session_reject_update();",
            )
            .unwrap();
        let mut paused = AssessmentSession::from_persisted_created(
            "ses_update_sink",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_update_sink", 1, SessionCommand::Activate)
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn load_and_command_queries_fail_closed_when_command_relation_is_missing() {
        use super::{
            apply_assessment_session_migration, load_assessment_session,
            persist_assessment_session_commands,
        };
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_command_missing CASCADE; \
                 CREATE SCHEMA assessment_session_command_missing; \
                 SET search_path TO assessment_session_command_missing;",
            )
            .unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_command_missing', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 ); \
                 DROP TABLE assessment_session_command;",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_command_missing"),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        let mut paused = AssessmentSession::from_persisted_created(
            "ses_command_missing",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_command_missing", 1, SessionCommand::Activate)
            .unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn command_insert_and_replay_lookup_fail_closed_from_the_library() {
        use super::{apply_assessment_session_migration, persist_assessment_session_commands};
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_command_sink CASCADE; \
                 CREATE SCHEMA assessment_session_command_sink; \
                 SET search_path TO assessment_session_command_sink;",
            )
            .unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_command_sink', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 ); \
                 CREATE FUNCTION assessment_session_command_reject_insert() RETURNS trigger \
                 LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'assessment_session_command insert sink'; END $$; \
                 CREATE TRIGGER assessment_session_command_reject_insert \
                 BEFORE INSERT ON assessment_session_command \
                 FOR EACH STATEMENT EXECUTE FUNCTION assessment_session_command_reject_insert();",
            )
            .unwrap();
        let mut paused = AssessmentSession::from_persisted_created(
            "ses_command_sink",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_command_sink", 1, SessionCommand::Activate)
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn command_replay_mismatch_is_conflicting_replay() {
        use super::{apply_assessment_session_migration, persist_assessment_session_commands};
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_command_replay_mismatch CASCADE; \
                 CREATE SCHEMA assessment_session_command_replay_mismatch; \
                 SET search_path TO assessment_session_command_replay_mismatch;",
            )
            .unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_command_replay_mismatch', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 ); \
                 INSERT INTO assessment_session_command (\
                     session_ref, command_ref, command_sequence, command_name, resulting_state\
                 ) VALUES (\
                     'ses_command_replay_mismatch', 'cmd_activate_replay', 1, 'activate', 'paused'\
                 );",
            )
            .unwrap();
        let mut paused = AssessmentSession::from_persisted_created(
            "ses_command_replay_mismatch",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_replay", 1, SessionCommand::Activate)
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn command_insert_maps_missing_relation_to_database_error() {
        use super::{
            apply_assessment_session_migration, insert_session_command,
            persist_assessment_session_commands,
        };
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_command_insert_missing CASCADE; \
                 CREATE SCHEMA assessment_session_command_insert_missing; \
                 SET search_path TO assessment_session_command_insert_missing;",
            )
            .unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_command_insert_missing', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 ); \
                 INSERT INTO assessment_session_command (\
                     session_ref, command_ref, command_sequence, command_name, resulting_state\
                 ) VALUES (\
                     'ses_command_insert_missing', 'cmd_activate_existing', 1, 'activate', 'active'\
                 );",
            )
            .unwrap();
        let mut paused = AssessmentSession::from_persisted_created(
            "ses_command_insert_missing",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        paused
            .apply_command("cmd_activate_conflict_seq", 1, SessionCommand::Activate)
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session_commands(&mut transaction, &paused),
            Err(AssessmentSessionPersistenceError::SequenceConflict)
        ));
        transaction.rollback().unwrap();
        client
            .batch_execute("DROP TABLE assessment_session_command;")
            .unwrap();
        let mut missing = client.transaction().unwrap();
        assert!(matches!(
            insert_session_command(
                &mut missing,
                "ses_command_insert_missing",
                paused.accepted_commands().first().unwrap(),
                1,
            ),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        missing.rollback().unwrap();
    }

    #[test]
    fn command_sequence_lookup_maps_missing_relation_to_database_error() {
        use super::lookup_command_ref_at_sequence;
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO assessment_session_sequence_missing;")
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            lookup_command_ref_at_sequence(&mut transaction, "ses_sequence_missing", 1),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn command_replay_lookup_maps_missing_relation_to_database_error() {
        use super::load_command_replay_row;
        use postgres::{Client, NoTls};

        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute("SET search_path TO assessment_session_replay_missing;")
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_command_replay_row(&mut transaction, "ses_replay_missing", "cmd_missing"),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn published_snapshot_start_matches_created_session_and_rejects_rebinding() {
        let snapshot = published_snapshot_for_session_match();
        let started = created_session_for_start_from_published_snapshot(
            "ses_snapshot_start_alpha",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            &snapshot,
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(started.session_ref(), "ses_snapshot_start_alpha");
        assert_eq!(started.locale(), "ko-KR");
        assert_eq!(started.state(), SessionState::Created);
        assert!(matches!(
            created_session_for_start_from_published_snapshot(
                "ses_snapshot_start_alpha",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &snapshot,
                "en-US",
                20_000,
            ),
            Err(AssessmentSessionStartError::LocaleMismatch)
        ));
        assert!(matches!(
            created_session_for_start_from_published_snapshot(
                "12",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                &snapshot,
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
    }

    #[test]
    fn stored_release_start_and_corrupt_identity_fail_closed_from_the_library() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_library_start CASCADE; \
                 CREATE SCHEMA assessment_session_library_start; \
                 SET search_path TO assessment_session_library_start;",
            )
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        apply_assessment_session_migration(&mut client).unwrap();

        let published = published_release_for_unpublished_memory_start();
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &published).unwrap();
        let (started, inserted) = start_created_assessment_session_from_stored_release(
            &mut transaction,
            "ses_library_stored_start",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(inserted, AssessmentSessionPersistenceDisposition::Inserted);
        assert_eq!(started.session_ref(), "ses_library_stored_start");
        let (replayed, disposition) = start_created_assessment_session_from_stored_release(
            &mut transaction,
            "ses_library_stored_start",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(
            disposition,
            AssessmentSessionPersistenceDisposition::Duplicate
        );
        assert_eq!(replayed.session_ref(), started.session_ref());
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "ses_library_stored_start",
                "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "release_big_five_ko_v1",
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::ConflictingReplay
            ))
        ));
        transaction.commit().unwrap();

        client
            .batch_execute(
                "ALTER TABLE assessment_session \
                     DROP CONSTRAINT assessment_session_participant_ref_check; \
                 INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_library_numeric_participant', '12', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'created', 20000\
                 );",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_numeric_participant"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ));
        transaction.rollback().unwrap();

        client
            .execute(
                "UPDATE instrument_release SET publication_state = 'suspended' WHERE release_ref = $1",
                &[&"release_big_five_ko_v1"],
            )
            .unwrap();
        client
            .batch_execute("ALTER TABLE assessment_session DROP COLUMN participant_ref;")
            .unwrap();
        let replay = AssessmentSession::from_persisted_created(
            "ses_library_stored_start",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            REPLAY_DIGEST,
            "ko-KR",
            20_000,
        )
        .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_assessment_session(&mut transaction, &replay),
            Err(AssessmentSessionPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    fn connect_isolated_library_schema(schema: &str) -> Client {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE; \
                 CREATE SCHEMA {schema}; \
                 SET search_path TO {schema};"
            ))
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        client
    }

    fn persist_activated_library_session(
        client: &mut Client,
        session_ref: &str,
        command_ref: &str,
    ) {
        let published = published_release_for_unpublished_memory_start();
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &published).unwrap();
        let (mut started, inserted) = start_created_assessment_session_from_stored_release(
            &mut transaction,
            session_ref,
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(inserted, AssessmentSessionPersistenceDisposition::Inserted);
        started
            .apply_command(command_ref, 1, SessionCommand::Activate)
            .unwrap();
        persist_assessment_session_commands(&mut transaction, &started).unwrap();
        transaction.commit().unwrap();
    }

    #[test]
    fn load_replays_commands_and_rejects_corrupt_history_from_the_library() {
        let mut client =
            connect_isolated_library_schema("assessment_session_library_load_commands");
        persist_activated_library_session(
            &mut client,
            "ses_library_load_commands",
            "cmd_activate_library_load",
        );

        let mut transaction = client.transaction().unwrap();
        let loaded = load_assessment_session(&mut transaction, "ses_library_load_commands")
            .unwrap()
            .expect("a persisted Activate must reload after restart");
        assert_eq!(loaded.state(), SessionState::Active);
        assert_eq!(loaded.accepted_commands().len(), 1);
        transaction.rollback().unwrap();

        client
            .batch_execute(
                "INSERT INTO assessment_session (\
                     session_ref, participant_ref, instrument_release_ref, \
                     instrument_version_ref, instrument_release_content_digest, \
                     locale, session_state, created_at_unix_ms\
                 ) VALUES (\
                     'ses_library_active_without_commands', 'ptc_eb1b318917d24ca0ac5153c37ff696c7', \
                     'release_big_five_ko_v1', 'instrument_version_big_five_ko_v1', \
                     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
                     'ko-KR', 'active', 20000\
                 );",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_active_without_commands"),
            Err(AssessmentSessionPersistenceError::UnsupportedStoredState)
        ));
        transaction.rollback().unwrap();

        client
            .execute(
                "UPDATE assessment_session_command \
                 SET command_name = $2, resulting_state = $3 \
                 WHERE session_ref = $1 AND command_sequence = 1",
                &[&"ses_library_load_commands", &"pause", &"paused"],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_commands"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ));
        transaction.rollback().unwrap();

        client
            .execute(
                "UPDATE assessment_session_command \
                 SET command_name = $2, resulting_state = $3 \
                 WHERE session_ref = $1 AND command_sequence = 1",
                &[&"ses_library_load_commands", &"activate", &"paused"],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_commands"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ));
        transaction.rollback().unwrap();

        client
            .execute(
                "UPDATE assessment_session_command \
                 SET command_name = $2, resulting_state = $3 \
                 WHERE session_ref = $1 AND command_sequence = 1",
                &[&"ses_library_load_commands", &"activate", &"active"],
            )
            .unwrap();
        client
            .execute(
                "UPDATE assessment_session SET session_state = $2 WHERE session_ref = $1",
                &[&"ses_library_load_commands", &"paused"],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_commands"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn load_rejects_unknown_command_and_out_of_range_values_from_the_library() {
        let mut client = connect_isolated_library_schema("assessment_session_library_load_range");
        persist_activated_library_session(
            &mut client,
            "ses_library_load_range",
            "cmd_activate_library_range",
        );

        client
            .batch_execute(
                "ALTER TABLE assessment_session_command \
                     DROP CONSTRAINT assessment_session_command_command_name_check; \
                 UPDATE assessment_session_command SET command_name = 'not_a_session_command' \
                 WHERE session_ref = 'ses_library_load_range' AND command_sequence = 1;",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_range"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ));
        transaction.rollback().unwrap();

        client
            .batch_execute(
                "UPDATE assessment_session_command SET command_name = 'activate' \
                 WHERE session_ref = 'ses_library_load_range' AND command_sequence = 1; \
                 ALTER TABLE assessment_session_command \
                     DROP CONSTRAINT assessment_session_command_command_sequence_check; \
                 UPDATE assessment_session_command SET command_sequence = -1 \
                 WHERE session_ref = 'ses_library_load_range';",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_range"),
            Err(AssessmentSessionPersistenceError::ValueOutOfRange)
        ));
        transaction.rollback().unwrap();

        client
            .batch_execute(
                "UPDATE assessment_session_command SET command_sequence = 1 \
                 WHERE session_ref = 'ses_library_load_range'; \
                 ALTER TABLE assessment_session \
                     DROP CONSTRAINT assessment_session_created_at_unix_ms_check; \
                 UPDATE assessment_session SET created_at_unix_ms = -1 \
                 WHERE session_ref = 'ses_library_load_range';",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            load_assessment_session(&mut transaction, "ses_library_load_range"),
            Err(AssessmentSessionPersistenceError::ValueOutOfRange)
        ));
        transaction.rollback().unwrap();
    }

    #[test]
    fn stored_release_start_replays_after_suspend_and_rejects_invalid_identity() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS assessment_session_library_stored_replay CASCADE; \
                 CREATE SCHEMA assessment_session_library_stored_replay; \
                 SET search_path TO assessment_session_library_stored_replay;",
            )
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        apply_assessment_session_migration(&mut client).unwrap();

        let published = published_release_for_unpublished_memory_start();
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &published).unwrap();
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "12",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "ses_library_stored_replay",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                "en-US",
                20_000,
            ),
            Err(AssessmentSessionStartError::LocaleMismatch)
        ));
        let (started, inserted) = start_created_assessment_session_from_stored_release(
            &mut transaction,
            "ses_library_stored_replay",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "ko-KR",
            20_000,
        )
        .unwrap();
        assert_eq!(inserted, AssessmentSessionPersistenceDisposition::Inserted);
        transaction.commit().unwrap();

        client
            .execute(
                "UPDATE instrument_release SET publication_state = 'suspended' WHERE release_ref = $1",
                &[&"release_big_five_ko_v1"],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        let (replayed, disposition) = start_created_assessment_session_from_stored_release(
            &mut transaction,
            "ses_library_stored_replay",
            "ptc_eb1b318917d24ca0ac5153c37ff696c7",
            "release_big_five_ko_v1",
            "ko-KR",
            20_000,
        )
        .expect("HTTP start must replay the exact stored session after later persist suspend");
        assert_eq!(
            disposition,
            AssessmentSessionPersistenceDisposition::Duplicate
        );
        assert_eq!(replayed.session_ref(), started.session_ref());
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "ses_library_stored_replay",
                "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "release_big_five_ko_v1",
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::ConflictingReplay
            ))
        ));
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                "ses_library_stored_replay_new",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                "ko-KR",
                21_000,
            ),
            Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
        ));
        transaction.rollback().unwrap();
    }
}
