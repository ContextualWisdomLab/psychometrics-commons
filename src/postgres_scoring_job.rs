//! `PostgreSQL` 18 persistence for asynchronous scoring-job ownership.
//!
//! This adapter persists product orchestration state only. Psychometric scoring remains
//! owned by `fast-mlsirm`. The caller owns the database connection, credentials, and
//! transaction boundary. Enqueue replay, worker claims, retry transitions, expired-lease
//! recovery, and terminal outcomes require `READ COMMITTED`; row locks and conditional
//! updates preserve stale-worker fencing and immutable completion evidence.

use crate::reference::canonical_opaque_reference;
use crate::scoring_job::{ScoringJob, ScoringJobState};
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const SCORING_JOB_MIGRATION: &str = include_str!("../migrations/0002_scoring_job_state.sql");

/// Outcome of persisting a scoring-job cancellation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobCancellationDisposition {
    /// A cancellable job was marked cancelled for the first time.
    Cancelled,
    /// The job was already cancelled and the request was replayed exactly.
    Duplicate,
}

/// Outcome of persisting the immutable identity of a scoring job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobPersistenceDisposition {
    /// The queued job identity was inserted for the first time.
    Inserted,
    /// The same immutable job identity already existed.
    Duplicate,
}

/// Outcome of persisting an immutable successful scoring completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobCompletionDisposition {
    /// The live fenced attempt was completed for the first time.
    Completed,
    /// The same immutable result and fencing evidence was replayed exactly.
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

    /// Return the caller-supplied lease expiry instant persisted with the claim.
    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

/// Fail-closed error for durable scoring-job persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringJobPersistenceError {
    /// A job, worker, lease, result, or failure identity was blank or numeric-like.
    InvalidReference,
    /// A caller-supplied timestamp was zero.
    InvalidTimestamp,
    /// A timestamp or counter cannot be represented by the bounded database column.
    ValueOutOfRange,
    /// A fencing token was zero and therefore cannot represent a persisted attempt.
    InvalidFencingToken,
    /// Lease expiry was not strictly later than the claim instant.
    InvalidLeaseWindow,
    /// Retry scheduling preceded the observed failure instant.
    InvalidRetryWindow,
    /// A retry-scheduled job was claimed before its persisted due time.
    LeaseNotDue,
    /// Only a fresh queued domain job can be inserted by this adapter.
    UnsupportedInitialState,
    /// Enqueue replay reused a job identity with different immutable evidence.
    ConflictingReplay,
    /// Completed immutable evidence was replayed with a different result or fence.
    ConflictingCompletion,
    /// Scoring-job persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// The requested scoring job does not exist.
    JobNotFound,
    /// The persisted job is not currently available for a new worker claim.
    NotLeaseable,
    /// A worker-side transition was submitted for a job without a current lease.
    NotLeased,
    /// A worker presented a fencing token that does not own the current lease.
    StaleLease,
    /// A worker-side transition was observed at or after persisted lease expiry.
    LeaseExpired,
    /// Expiry recovery was requested while the persisted lease is still live.
    LeaseStillActive,
    /// A guarded terminal transition was suppressed after its lease evidence was validated.
    TransitionNotApplied,
    /// Completed or quarantined jobs cannot be rewritten as cancelled.
    TerminalState,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ScoringJobPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "scoring persistence references must be opaque values",
            Self::InvalidTimestamp => "scoring persistence timestamps must be greater than zero",
            Self::ValueOutOfRange => "scoring persistence value exceeds the PostgreSQL range",
            Self::InvalidFencingToken => "scoring persistence fencing tokens must be positive",
            Self::InvalidLeaseWindow => "scoring lease expiry must be later than claim time",
            Self::InvalidRetryWindow => "scoring retry time cannot precede failure time",
            Self::LeaseNotDue => "scoring retry is not yet due for another lease",
            Self::UnsupportedInitialState => "only a fresh queued scoring job may be inserted",
            Self::ConflictingReplay => {
                "scoring job identity was replayed with conflicting evidence"
            }
            Self::ConflictingCompletion => {
                "scoring completion was replayed with conflicting immutable evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "scoring job persistence requires read committed isolation"
            }
            Self::JobNotFound => "scoring job does not exist",
            Self::NotLeaseable => "scoring job is not currently leaseable",
            Self::NotLeased => "scoring job does not currently have a worker lease",
            Self::StaleLease => "scoring worker fencing token is stale",
            Self::LeaseExpired => "scoring worker lease has expired",
            Self::LeaseStillActive => "scoring job lease has not expired",
            Self::TransitionNotApplied => "scoring terminal transition was not applied",
            Self::TerminalState => "completed or quarantined scoring jobs cannot be cancelled",
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
        (
            job.state(),
            job.attempt_count(),
            job.active_lease(),
            job.result_ref()
        ),
        (ScoringJobState::Queued, 0, None, None)
    ) {
        return Err(ScoringJobPersistenceError::UnsupportedInitialState);
    }
    let max_attempts = i32::try_from(job.max_attempts())
        .map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)?;
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

/// Atomically claim one queued or due retry-scheduled scoring job for a worker.
///
/// One conditional `UPDATE` changes the due job to `leased`, increments the persisted
/// attempt count, and binds worker/lease/fencing plus caller-supplied expiry evidence in
/// the same row lock. Concurrent claimers cannot both receive ownership. A retry-scheduled
/// row is leaseable only when `claimed_at_unix_ms` is at or after its persisted due time.
/// `READ COMMITTED` is required so failed-claim classification observes the latest committed
/// state rather than relying on a transaction-fixed snapshot.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for invalid references/timestamps, an invalid
/// lease window, unsupported isolation, an unknown/non-leaseable job, an early retry claim,
/// out-of-range database values, or a database failure.
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
    require_read_committed(transaction)?;

    let claimed = transaction.query_opt(
        "UPDATE scoring_job_state \
         SET scoring_state = 'leased',\
             attempt_count = attempt_count + 1,\
             next_attempt_at_unix_ms = NULL,\
             last_failure_code = NULL,\
             active_worker_ref = $2,\
             active_lease_ref = $3,\
             active_fencing_token = attempt_count + 1,\
             active_lease_expires_at_unix_ms = $4,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND (\
               scoring_state = 'queued' \
               OR (\
                   scoring_state = 'retry_scheduled' \
                   AND next_attempt_at_unix_ms <= $5\
               )\
           ) \
           AND attempt_count < max_attempts \
         RETURNING attempt_count, active_fencing_token",
        &[
            &scoring_job_ref,
            &worker_ref,
            &lease_ref,
            &expires_at_unix_ms,
            &claimed_at_unix_ms,
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

    let row = transaction.query_opt(
        "SELECT COALESCE(\
             scoring_state = 'retry_scheduled' \
             AND next_attempt_at_unix_ms > $2,\
             FALSE\
         ) AS lease_not_due \
         FROM scoring_job_state WHERE scoring_job_ref = $1",
        &[&scoring_job_ref, &claimed_at_unix_ms],
    )?;
    let Some(row) = row else {
        return Err(ScoringJobPersistenceError::JobNotFound);
    };
    let lease_not_due: bool = row.get(0);
    if lease_not_due {
        Err(ScoringJobPersistenceError::LeaseNotDue)
    } else {
        Err(ScoringJobPersistenceError::NotLeaseable)
    }
}

/// Recover one expired leased scoring job into a due retry or quarantine.
///
/// Expiry never assigns another worker the expired fencing token. The row becomes
/// `retry_scheduled` at the observation instant when attempt budget remains, or
/// `quarantined` when the attempt is exhausted. A later [`claim_scoring_job`] issues
/// the next fence. Fallback classification locks the current row until the caller
/// commits so a concurrent worker cannot rewrite unleased evidence. `READ COMMITTED`
/// is required so concurrent expiry recovery observes the latest committed lease.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for an invalid identity or timestamp,
/// unsupported isolation, a missing or unleased job, a still-live lease, or a
/// database failure.
pub fn expire_scoring_lease(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    observed_at_unix_ms: u64,
) -> Result<ScoringJobState, ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let observed_at_unix_ms = postgres_timestamp(observed_at_unix_ms)?;
    require_read_committed(transaction)?;

    let recovered = transaction.query_opt(
        "UPDATE scoring_job_state \
         SET scoring_state = CASE \
                 WHEN attempt_count >= max_attempts THEN 'quarantined' \
                 ELSE 'retry_scheduled' \
             END,\
             next_attempt_at_unix_ms = CASE \
                 WHEN attempt_count >= max_attempts THEN NULL \
                 ELSE $2::BIGINT \
             END,\
             last_failure_code = 'lease_expired',\
             active_worker_ref = NULL,\
             active_lease_ref = NULL,\
             active_fencing_token = NULL,\
             active_lease_expires_at_unix_ms = NULL,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND scoring_state = 'leased' \
           AND active_lease_expires_at_unix_ms <= $2 \
         RETURNING scoring_state",
        &[&scoring_job_ref, &observed_at_unix_ms],
    )?;
    if let Some(row) = recovered {
        let state: String = row.get(0);
        return Ok(if state == "quarantined" {
            ScoringJobState::Quarantined
        } else {
            ScoringJobState::RetryScheduled
        });
    }

    let row = match transaction.query_opt(
        "SELECT scoring_state = 'leased' AS leased
         FROM scoring_job_state
         WHERE scoring_job_ref = $1
         FOR UPDATE",
        &[&scoring_job_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(ScoringJobPersistenceError::from(error)),
    };
    let Some(row) = row else {
        return Err(ScoringJobPersistenceError::JobNotFound);
    };
    let leased: bool = row.get(0);
    if leased {
        Err(ScoringJobPersistenceError::LeaseStillActive)
    } else {
        Err(ScoringJobPersistenceError::NotLeased)
    }
}

/// Cancel queued, leased, or retry-scheduled scoring work without transferring a fence.
///
/// Cancellation clears any active lease and due-retry instant. Exact replay of an
/// already cancelled job is idempotent. Completed or quarantined evidence cannot be
/// rewritten. Fallback classification locks the current row until the caller commits
/// so a concurrent worker cannot rewrite terminal evidence. `READ COMMITTED` is
/// required so concurrent completion is visible.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for an invalid identity, unsupported
/// isolation, a missing job, a completed or quarantined job, a suppressed
/// transition, or a database failure.
pub fn cancel_scoring_job(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
) -> Result<ScoringJobCancellationDisposition, ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    require_read_committed(transaction)?;

    let cancelled = match transaction.query_opt(
        "UPDATE scoring_job_state
         SET scoring_state = 'cancelled',
             next_attempt_at_unix_ms = NULL,
             active_worker_ref = NULL,
             active_lease_ref = NULL,
             active_fencing_token = NULL,
             active_lease_expires_at_unix_ms = NULL,
             updated_at = clock_timestamp()
         WHERE scoring_job_ref = $1
           AND scoring_state IN ('queued', 'leased', 'retry_scheduled')
         RETURNING scoring_job_ref",
        &[&scoring_job_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(ScoringJobPersistenceError::from(error)),
    };
    if cancelled.is_some() {
        return Ok(ScoringJobCancellationDisposition::Cancelled);
    }

    let row = match transaction.query_opt(
        "SELECT scoring_state
         FROM scoring_job_state
         WHERE scoring_job_ref = $1
         FOR UPDATE",
        &[&scoring_job_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(ScoringJobPersistenceError::from(error)),
    };
    let Some(row) = row else {
        return Err(ScoringJobPersistenceError::JobNotFound);
    };
    let state: String = row.get(0);
    match state.as_str() {
        "cancelled" => Ok(ScoringJobCancellationDisposition::Duplicate),
        "completed" | "quarantined" => Err(ScoringJobPersistenceError::TerminalState),
        _ => Err(ScoringJobPersistenceError::TransitionNotApplied),
    }
}

/// Persist a retryable failure for the worker that owns the current fenced lease.
///
/// The transition locks the current scoring-job row, validates state, fencing token, and
/// persisted lease expiry, then updates the row inside the same caller-owned transaction.
/// When attempt budget remains, the row becomes `retry_scheduled` at the requested future
/// instant; exhausting the budget moves it directly to `quarantined`. In both cases the
/// former lease evidence is cleared atomically so a stale worker cannot retain ownership.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] when references/timestamps/fencing evidence are
/// invalid, retry time precedes failure time, isolation is unsupported, the job is missing
/// or no longer leased, a stale fence is presented, the lease already expired, or the
/// database operation fails.
pub fn record_retryable_scoring_failure(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    cause_code: &str,
    failed_at_unix_ms: u64,
    retry_at_unix_ms: u64,
) -> Result<ScoringJobState, ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let cause_code = required_reference(cause_code)?;
    let fencing_token = postgres_fencing_token(fencing_token)?;
    let failed_at_unix_ms = postgres_timestamp(failed_at_unix_ms)?;
    let retry_at_unix_ms = postgres_timestamp(retry_at_unix_ms)?;
    if retry_at_unix_ms < failed_at_unix_ms {
        return Err(ScoringJobPersistenceError::InvalidRetryWindow);
    }
    require_read_committed(transaction)?;
    require_current_scoring_lease(
        transaction,
        scoring_job_ref,
        fencing_token,
        failed_at_unix_ms,
    )?;

    let transitioned = transaction.query_one(
        "UPDATE scoring_job_state \
         SET scoring_state = CASE \
                 WHEN attempt_count >= max_attempts THEN 'quarantined' \
                 ELSE 'retry_scheduled' \
             END,\
             next_attempt_at_unix_ms = CASE \
                 WHEN attempt_count >= max_attempts THEN NULL \
                 ELSE $4::BIGINT \
             END,\
             last_failure_code = $3,\
             active_worker_ref = NULL,\
             active_lease_ref = NULL,\
             active_fencing_token = NULL,\
             active_lease_expires_at_unix_ms = NULL,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND scoring_state = 'leased' \
           AND active_fencing_token = $2 \
           AND active_lease_expires_at_unix_ms > $5 \
         RETURNING attempt_count >= max_attempts AS quarantined",
        &[
            &scoring_job_ref,
            &fencing_token,
            &cause_code,
            &retry_at_unix_ms,
            &failed_at_unix_ms,
        ],
    )?;

    let quarantined: bool = transitioned.get(0);
    Ok(if quarantined {
        ScoringJobState::Quarantined
    } else {
        ScoringJobState::RetryScheduled
    })
}

/// Persist a permanent failure for the worker that owns the current fenced lease.
///
/// Permanent failure never schedules another automatic attempt. The job is quarantined,
/// typed failure evidence is retained, no scoring result is fabricated, and all active
/// lease evidence is cleared in the same row-locked transaction.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for invalid evidence, unsupported isolation,
/// a missing/non-leased job, stale or expired worker authority, a suppressed terminal
/// transition, or a database failure.
pub fn record_permanent_scoring_failure(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    cause_code: &str,
    failed_at_unix_ms: u64,
) -> Result<(), ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let cause_code = required_reference(cause_code)?;
    let fencing_token = postgres_fencing_token(fencing_token)?;
    let failed_at_unix_ms = postgres_timestamp(failed_at_unix_ms)?;
    require_read_committed(transaction)?;
    require_current_scoring_lease(
        transaction,
        scoring_job_ref,
        fencing_token,
        failed_at_unix_ms,
    )?;

    let updated = transaction.execute(
        "UPDATE scoring_job_state \
         SET scoring_state = 'quarantined',\
             next_attempt_at_unix_ms = NULL,\
             last_failure_code = $3,\
             active_worker_ref = NULL,\
             active_lease_ref = NULL,\
             active_fencing_token = NULL,\
             active_lease_expires_at_unix_ms = NULL,\
             result_ref = NULL,\
             completed_fencing_token = NULL,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND scoring_state = 'leased' \
           AND active_fencing_token = $2 \
           AND active_lease_expires_at_unix_ms > $4",
        &[
            &scoring_job_ref,
            &fencing_token,
            &cause_code,
            &failed_at_unix_ms,
        ],
    )?;
    if updated != 1 {
        return Err(ScoringJobPersistenceError::TransitionNotApplied);
    }
    Ok(())
}

/// Persist one immutable successful scoring result for the current fenced attempt.
///
/// The first completion is accepted only while the presenting lease is live. Exact replay
/// of the already accepted result and fencing token is idempotent even after the original
/// lease window has elapsed. Any different result or fencing token fails closed rather than
/// rewriting historical scoring evidence.
///
/// # Errors
///
/// Returns [`ScoringJobPersistenceError`] for invalid evidence, unsupported isolation,
/// a missing/non-leased job, stale or expired worker authority, conflicting completion,
/// a suppressed terminal transition, or a database failure.
pub fn record_successful_scoring_completion(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    scoring_result_ref: &str,
    completed_at_unix_ms: u64,
) -> Result<ScoringJobCompletionDisposition, ScoringJobPersistenceError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let scoring_result_ref = required_reference(scoring_result_ref)?;
    let fencing_token = postgres_fencing_token(fencing_token)?;
    let completed_at_unix_ms = postgres_timestamp(completed_at_unix_ms)?;
    require_read_committed(transaction)?;

    let row = transaction.query_opt(
        "SELECT scoring_state, result_ref, completed_fencing_token,\
                active_fencing_token, active_lease_expires_at_unix_ms \
         FROM scoring_job_state WHERE scoring_job_ref = $1 \
         FOR UPDATE",
        &[&scoring_job_ref],
    )?;
    let Some(row) = row else {
        return Err(ScoringJobPersistenceError::JobNotFound);
    };

    let scoring_state: String = row.get(0);
    let stored_result_ref: Option<String> = row.get(1);
    let completed_fencing_token: Option<i64> = row.get(2);
    if scoring_state == "completed" {
        if stored_result_ref.as_deref() == Some(scoring_result_ref)
            && completed_fencing_token == Some(fencing_token)
        {
            return Ok(ScoringJobCompletionDisposition::Duplicate);
        }
        return Err(ScoringJobPersistenceError::ConflictingCompletion);
    }
    if scoring_state != "leased" {
        return Err(ScoringJobPersistenceError::NotLeased);
    }

    let active_fencing_token: Option<i64> = row.get(3);
    if active_fencing_token != Some(fencing_token) {
        return Err(ScoringJobPersistenceError::StaleLease);
    }
    let active_lease_expires_at_unix_ms: Option<i64> = row.get(4);
    if !matches!(
        active_lease_expires_at_unix_ms,
        Some(expiry) if expiry > completed_at_unix_ms
    ) {
        return Err(ScoringJobPersistenceError::LeaseExpired);
    }

    let updated = transaction.execute(
        "UPDATE scoring_job_state \
         SET scoring_state = 'completed',\
             next_attempt_at_unix_ms = NULL,\
             last_failure_code = NULL,\
             active_worker_ref = NULL,\
             active_lease_ref = NULL,\
             active_fencing_token = NULL,\
             active_lease_expires_at_unix_ms = NULL,\
             result_ref = $3,\
             completed_fencing_token = $2,\
             updated_at = clock_timestamp() \
         WHERE scoring_job_ref = $1 \
           AND scoring_state = 'leased' \
           AND active_fencing_token = $2 \
           AND active_lease_expires_at_unix_ms > $4",
        &[
            &scoring_job_ref,
            &fencing_token,
            &scoring_result_ref,
            &completed_at_unix_ms,
        ],
    )?;
    if updated != 1 {
        return Err(ScoringJobPersistenceError::TransitionNotApplied);
    }
    Ok(ScoringJobCompletionDisposition::Completed)
}

fn require_current_scoring_lease(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: i64,
    observed_at_unix_ms: i64,
) -> Result<(), ScoringJobPersistenceError> {
    let row = transaction.query_opt(
        "SELECT scoring_state = 'leased' AS leased,\
                COALESCE(active_fencing_token = $2, FALSE) AS current_fence,\
                COALESCE(active_lease_expires_at_unix_ms > $3, FALSE) AS lease_unexpired \
         FROM scoring_job_state WHERE scoring_job_ref = $1 \
         FOR UPDATE",
        &[&scoring_job_ref, &fencing_token, &observed_at_unix_ms],
    )?;
    let Some(row) = row else {
        return Err(ScoringJobPersistenceError::JobNotFound);
    };
    let leased: bool = row.get(0);
    if !leased {
        return Err(ScoringJobPersistenceError::NotLeased);
    }
    let current_fence: bool = row.get(1);
    if !current_fence {
        return Err(ScoringJobPersistenceError::StaleLease);
    }
    let lease_unexpired: bool = row.get(2);
    if !lease_unexpired {
        return Err(ScoringJobPersistenceError::LeaseExpired);
    }
    Ok(())
}

fn required_reference(reference: &str) -> Result<&str, ScoringJobPersistenceError> {
    canonical_opaque_reference(reference).ok_or(ScoringJobPersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, ScoringJobPersistenceError> {
    if timestamp == 0 {
        return Err(ScoringJobPersistenceError::InvalidTimestamp);
    }
    i64::try_from(timestamp).map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)
}

fn postgres_fencing_token(fencing_token: u64) -> Result<i64, ScoringJobPersistenceError> {
    if fencing_token == 0 {
        return Err(ScoringJobPersistenceError::InvalidFencingToken);
    }
    i64::try_from(fencing_token).map_err(|_| ScoringJobPersistenceError::ValueOutOfRange)
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
