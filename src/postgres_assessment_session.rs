//! `PostgreSQL` 18 persistence for assessment-session creation identity.
//!
//! This module stores the participant, published-release, version, content-digest,
//! and locale identity copied at session creation. It does not rewrite provenance
//! when the release is later suspended or retired. This first slice persists
//! only [`SessionState::Created`] rows. Load restores that created identity
//! without re-checking current publication eligibility. Replay requires
//! `READ COMMITTED`.

use crate::reference::normalized_reference;
use crate::session::{AssessmentSession, SessionState};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const ASSESSMENT_SESSION_MIGRATION: &str =
    include_str!("../migrations/0014_assessment_session.sql");

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
    /// Stored session state is not created; this slice cannot load later states.
    UnsupportedStoredState,
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

/// Apply the idempotent assessment-session migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_assessment_session_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ASSESSMENT_SESSION_MIGRATION)
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
    let session_state = session_state_name(session.state());
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

/// Load one created assessment-session identity without a live published release.
///
/// Missing rows return [`None`]. A stored later lifecycle state fails closed. Exact
/// stored identity is restored through [`AssessmentSession::from_persisted_created`],
/// so a later suspend or retire cannot rewrite provenance. Command history is not
/// stored in this slice.
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
    if stored_state != session_state_name(SessionState::Created) {
        return Err(AssessmentSessionPersistenceError::UnsupportedStoredState);
    }
    let stored_created_at: i64 = row.get(6);
    let created_at_unix_ms = u64::try_from(stored_created_at)
        .map_err(|_| AssessmentSessionPersistenceError::ValueOutOfRange)?;
    AssessmentSession::from_persisted_created(
        session_ref,
        row.get::<_, String>(0).as_str(),
        row.get::<_, String>(1).as_str(),
        row.get::<_, String>(2).as_str(),
        row.get::<_, String>(3).as_str(),
        row.get::<_, String>(4).as_str(),
        created_at_unix_ms,
    )
    .map(Some)
    .map_err(|_| AssessmentSessionPersistenceError::InvalidStoredIdentity)
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
        && stored_state == session_state_name(session.state())
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

/// Map the domain session state to its stable persisted vocabulary.
fn session_state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Created => "created",
        SessionState::Active => "active",
        SessionState::Paused => "paused",
        SessionState::Completed => "completed",
        SessionState::Scoring => "scoring",
        SessionState::Scored => "scored",
        SessionState::Released => "released",
        SessionState::Expired => "expired",
        SessionState::Cancelled => "cancelled",
        SessionState::Invalidated => "invalidated",
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
    use super::{session_state_name, AssessmentSessionPersistenceError};
    use crate::session::SessionState;

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
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn session_state_names_cover_the_lifecycle_vocabulary() {
        assert_eq!(session_state_name(SessionState::Created), "created");
        assert_eq!(session_state_name(SessionState::Active), "active");
        assert_eq!(session_state_name(SessionState::Paused), "paused");
        assert_eq!(session_state_name(SessionState::Completed), "completed");
        assert_eq!(session_state_name(SessionState::Scoring), "scoring");
        assert_eq!(session_state_name(SessionState::Scored), "scored");
        assert_eq!(session_state_name(SessionState::Released), "released");
        assert_eq!(session_state_name(SessionState::Expired), "expired");
        assert_eq!(session_state_name(SessionState::Cancelled), "cancelled");
        assert_eq!(session_state_name(SessionState::Invalidated), "invalidated");
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
