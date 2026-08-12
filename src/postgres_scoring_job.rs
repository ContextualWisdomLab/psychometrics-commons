//! `PostgreSQL` 18 persistence for asynchronous scoring-job ownership.
//!
//! This adapter persists product orchestration state only. Psychometric scoring remains
//! owned by `fast-mlsirm`. The caller owns the database connection, credentials, and
//! transaction boundary. Enqueue replay is checked under `READ COMMITTED`, while worker
//! claim uses one conditional `UPDATE` so only one transaction can acquire a queued job.

use crate::reference::normalized_reference;
use crate::scoring_job::{ScoringJob, ScoringJobState};
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const SCORING_JOB_MIGRATION: &str = include_str!("../migrations/0002_scoring_job_state.sql");
const QUEUED_STATE: &str = "queued";
const LEASED_STATE: &str = "leased";

/// Outcome of persisting the immutable identity of a scoring job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobPersistenceDisposition {
    /// The queued job identity was inserted for the first time.
    Inserted,
    /// The same immutable job identity already existed.
    Duplicate,
}

/// Durable ownership evidence returned by an atomic worker claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedScoringLease {
    worker_ref: String,
    lease_ref: String,
    fencing_token: u64,
    expires_at_unix_ms: u64,
}

impl PersistedScoringLease {
    /// Return the opaque worker identity that owns this persisted lease.
    #[must_use]
    pub fn worker_ref(&self) -> &str {
        &self.worker_ref
    }

    /// Return the opaque lease identity for this persisted attempt.
    #[must_use]
    pub fn lease_ref(&self) -> &str {
        &self.lease_ref
    }

    /// Return the monotonically increasing stale-worker fencing token.
    #[must_use]
    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    /// Return the server-authoritative lease expiry instant.
    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

/// Fail-closed error for the first durable scoring-job persistence slice.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringJobPersistenceError {
    /// A job, worker, or lease identity was blank or numeric-like.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// A timestamp or counter cannot be represented by the bounded database column.
    ValueOutOfRange,
    /// Lease expiry was not strictly later than the claim instant.
    InvalidLeaseWindow,
    /// Only a fresh queued domain job can be inserted by this first-slice adapter.
    UnsupportedInitialState,
    /// Enqueue replay reused a job identity with different immutable evidence.
    ConflictingReplay,
    /// Enqueue replay classification requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// The requested scoring job does not exist.
    JobNotFound,
    /// The persisted job is not currently available for a new worker claim.
    NotLeaseable,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ScoringJobPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "scoring persistence references must be opaque values",
            Self::InvalidTimestamp => "scoring persistence timestamps must be greater than zero",
            Self::ValueOutOfRange => "scoring persistence value exceeds the PostgreSQL range",
            Self::InvalidLeaseWindow => "scoring lease expiry must be later than claim time",
            Self::UnsupportedInitialState => "only a fresh queued scoring job may be inserted",
            Self::ConflictingReplay => "scoring job identity was replayed with conflicting evidence",
            Self::UnsupportedIsolationLevel => {
                "scoring job enqueue replay requires read committed isolation"
            }
            Self::JobNotFound => "scoring job does not exist",
            Self::NotLeaseable => "scoring job is not currently leaseable",
            Self::Database(_) => "PostgreSQL scoring-job persistence failed",
        })
    }
}

impl Error for ScoringJobPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ScoringJobPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent scoring-job migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_scoring_job_migration(client: &mut impl GenericClient) -> Result<(), postgres::Error> {
    client.batch_execute(SCORING_JOB_MIGRATION)
}

/// Persist the immutable identity of a fresh queued scoring job.
///
/// Exact replay is idempotent even if the mutable job row has since advanced. Reusing
/// `scoring_job_ref` for a different scoring request or attempt budget fails closed.
/// The insert-then-inspect classifier requires `READ COMMITTED` so a concurrent insert
/// that wins the unique-key race is visible to the replay-classification statement.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for a non-fresh domain state, unsupported
/// isolation, an out-of-range attempt budget, conflicting replay, or a database failure.
pub fn persist_scoring_job(
    transaction: &mut Transaction<'_>,
    job: &ScoringJob,
) -> Result<ScoringJobPersistenceDisposition, ScoringJobPersistenceError> {
    if !matches!(
        (job.state(), job.attempt_count(), job.active_lease(), job.result_ref()),
        (ScoringJobState::Queued, 0, None, None)
    ) {
        return Err(ScoringJobPersistenceError::UnsupportedInitialState);
    }
    let max_attempts =
        i32::try_from(job.max_attempts()).map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)?;
    require_read_committed(transaction)?;

    let inserted = transaction.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts\
         ) VALUES ($1, $2, 'queued', 0, $3) \
         ON CONFLICT (scoring_job_ref) DO NOTHING",
        &[
            &job.scoring_job_ref(),
            &job.scoring_request_ref(),
            &max_attempts,
        ],
    )?;
    if inserted == 1 {
        return Ok(ScoringJobPersistenceDisposition::Inserted);
    }

    let row = transaction.query_one(
        "SELECT scoring_request_ref, max_attempts \
         FROM scoring_job_state WHERE scoring_job_ref = $1",
        &[&job.scoring_job_ref()],
    )?;
    let stored_request_ref: String = row.get(0);
    let stored_max_attempts: i32 = row.get(1);
    if stored_request_ref == job.scoring_request_ref() && stored_max_attempts == max_attempts {
        Ok(ScoringJobPersistenceDisposition::Duplicate)
    } else {
        Err(ScoringJobPersistenceError::ConflictingReplay)
    }
}

/// Atomically claim one queued scoring job for a worker.
///
/// One conditional `UPDATE` changes `queued -> leased`, increments the persisted attempt
/// count, and binds worker/lease/fencing/expiry evidence in the same row lock. Concurrent
/// claimers cannot both receive ownership. This first slice intentionally does not recover
/// expired leases or persist retry/completion transitions; those remain separate bounded
/// follow-up work so the adapter cannot claim recovery semantics it does not implement.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for invalid references/timestamps, an invalid
/// lease window, an unknown/non-leaseable job, out-of-range database values, or a database
/// failure.
pub fn claim_scoring_job(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    worker_ref: &str,
    lease_ref: &str,
    claimed_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<PersistedScoringLease, ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let worker_ref = required_reference(worker_ref)?;
    let lease_ref = required_reference(lease_ref)?;
    let claimed_at_unix_ms = postgres_timestamp(claimed_at_unix_ms)?;
    let expires_at_unix_ms = postgres_timestamp(expires_at_unix_ms)?;
    if expires_at_unix_ms <= claimed_at_unix_ms {
        return Err(ScoringJobPersistenceError::InvalidLeaseWindow);
    }

    let claimed = transaction.query_opt(
        "UPDATE scoring_job_state \
         SET scoring_state = 'leased',\
             attempt_count = attempt_count + 1,\
             active_worker_ref = $2,\
             active_lease_ref = $3,\
             active_fencing_token = attempt_count + 1,\
             active_lease_expires_at_unix_ms = $4,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND scoring_state = 'queued' \
           AND attempt_count < max_attempts \
         RETURNING attempt_count, active_fencing_token",
        &[
            &scoring_job_ref,
            &worker_ref,
            &lease_ref,
            &expires_at_unix_ms,
        ],
    )?;

    if let Some(row) = claimed {
        let attempt_count: i32 = row.get(0);
        let fencing_token: i64 = row.get(1);
        debug_assert_eq!(i64::from(attempt_count), fencing_token);
        return Ok(PersistedScoringLease {
            worker_ref: worker_ref.to_owned(),
            lease_ref: lease_ref.to_owned(),
            fencing_token: u64::try_from(fencing_token)
                .map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)?,
            expires_at_unix_ms: u64::try_from(expires_at_unix_ms)
                .map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)?,
        });
    }

    let exists = transaction
        .query_opt(
            "SELECT scoring_state FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&scoring_job_ref],
        )?
        .is_some();
    if exists {
        Err(ScoringJobPersistenceError::NotLeaseable)
    } else {
        Err(ScoringJobPersistenceError::JobNotFound)
    }
}

fn required_reference(reference: &str) -> Result<&str, ScoringJobPersistenceError> {
    normalized_reference(reference).ok_or(ScoringJobPersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, ScoringJobPersistenceError> {
    if timestamp == 0 {
        return Err(ScoringJobPersistenceError::InvalidTimestamp);
    }
    i64::try_from(timestamp).map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ScoringJobPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ScoringJobPersistenceError::UnsupportedIsolationLevel)
    }
}

#[allow(dead_code)]
const _: (&str, &str) = (QUEUED_STATE, LEASED_STATE);
