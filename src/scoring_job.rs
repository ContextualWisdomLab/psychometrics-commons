//! Asynchronous scoring-job lifecycle with bounded retry and lease fencing.
//!
//! This module owns product orchestration state only. It does not calculate
//! psychometric quantities, call a scoring engine, or persist worker leases.
//! Persistence adapters must preserve these state, attempt, time-bound lease,
//! and fencing invariants with real database concurrency evidence.

use crate::reference::canonical_opaque_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

const LEASE_EXPIRED_CAUSE: &str = "lease_expired";

/// Server-authoritative lifecycle state for one asynchronous scoring job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobState {
    /// The job is ready for its first worker lease.
    Queued,
    /// One worker currently owns a time-bounded fenced lease.
    Leased,
    /// A retryable attempt failed or expired and a later attempt is scheduled.
    RetryScheduled,
    /// One immutable scoring result was accepted for the job.
    Completed,
    /// Automatic processing stopped and operator/scientific reconciliation is required.
    Quarantined,
    /// The product cancelled the job and any former worker lease is invalid.
    Cancelled,
}

/// Immutable ownership evidence issued for one scoring attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoringLease {
    worker_ref: String,
    lease_ref: String,
    fencing_token: u64,
    expires_at_unix_ms: u64,
}

impl ScoringLease {
    /// Return the opaque worker identity that owns this lease.
    #[must_use]
    pub fn worker_ref(&self) -> &str {
        &self.worker_ref
    }

    /// Return the opaque lease identity for this attempt.
    #[must_use]
    pub fn lease_ref(&self) -> &str {
        &self.lease_ref
    }

    /// Return the monotonically increasing token that fences stale workers.
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

/// Product-owned asynchronous scoring job bound to one immutable scoring request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoringJob {
    job_ref: String,
    scoring_request_ref: String,
    state: ScoringJobState,
    attempt_count: u32,
    max_attempts: u32,
    next_attempt_at_unix_ms: u64,
    last_failure_code: Option<String>,
    active_lease: Option<ScoringLease>,
    result_ref: Option<String>,
    completed_fencing_token: Option<u64>,
}

impl ScoringJob {
    /// Create a queued scoring job for one immutable scoring request.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError::InvalidReference`] for blank, numeric-like,
    /// or non-exact identity spelling and [`ScoringJobError::InvalidAttemptLimit`]
    /// when `max_attempts` is zero.
    pub fn new(
        scoring_job_ref: impl Into<String>,
        scoring_request_ref: impl Into<String>,
        max_attempts: u32,
    ) -> Result<Self, ScoringJobError> {
        let scoring_job_ref = scoring_job_ref.into();
        let scoring_job_ref = required_reference(&scoring_job_ref)?;
        let scoring_request_ref = scoring_request_ref.into();
        let scoring_request_ref = required_reference(&scoring_request_ref)?;
        if max_attempts == 0 {
            return Err(ScoringJobError::InvalidAttemptLimit);
        }

        Ok(Self {
            job_ref: scoring_job_ref.to_owned(),
            scoring_request_ref: scoring_request_ref.to_owned(),
            state: ScoringJobState::Queued,
            attempt_count: 0,
            max_attempts,
            next_attempt_at_unix_ms: 0,
            last_failure_code: None,
            active_lease: None,
            result_ref: None,
            completed_fencing_token: None,
        })
    }

    /// Return the opaque scoring-job reference.
    #[must_use]
    pub fn scoring_job_ref(&self) -> &str {
        &self.job_ref
    }

    /// Return the immutable scoring-request reference executed by this job.
    #[must_use]
    pub fn scoring_request_ref(&self) -> &str {
        &self.scoring_request_ref
    }

    /// Return the current server-authoritative job state.
    #[must_use]
    pub const fn state(&self) -> ScoringJobState {
        self.state
    }

    /// Return how many worker attempts have received a lease.
    #[must_use]
    pub const fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    /// Return the maximum number of automatic attempts.
    #[must_use]
    pub const fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    /// Return the earliest time at which a retry may be leased.
    #[must_use]
    pub const fn next_attempt_at_unix_ms(&self) -> Option<u64> {
        if self.next_attempt_at_unix_ms == 0 {
            None
        } else {
            Some(self.next_attempt_at_unix_ms)
        }
    }

    /// Return the latest typed failure cause retained for reconciliation.
    #[must_use]
    pub fn last_failure_code(&self) -> Option<&str> {
        self.last_failure_code.as_deref()
    }

    /// Return the current immutable lease evidence, when a worker owns the job.
    #[must_use]
    pub const fn active_lease(&self) -> Option<&ScoringLease> {
        self.active_lease.as_ref()
    }

    /// Return the immutable accepted scoring-result reference after completion.
    #[must_use]
    pub fn result_ref(&self) -> Option<&str> {
        self.result_ref.as_deref()
    }

    /// Claim a due job with a bounded worker lease.
    ///
    /// Each successful claim increments the attempt count and issues a strictly
    /// increasing fencing token. A worker must present that token for any later
    /// success/failure command, so a former lease cannot complete a newer attempt.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError`] when references/timestamps are invalid, the
    /// lease window is empty, a scheduled retry is not due, or the job cannot be
    /// leased from its current state.
    pub fn claim(
        &mut self,
        worker_ref: &str,
        lease_ref: &str,
        claimed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<ScoringLease, ScoringJobError> {
        let worker_ref = required_reference(worker_ref)?;
        let lease_ref = required_reference(lease_ref)?;
        require_timestamp(claimed_at_unix_ms)?;
        require_timestamp(expires_at_unix_ms)?;
        if expires_at_unix_ms <= claimed_at_unix_ms {
            return Err(ScoringJobError::InvalidLeaseWindow);
        }

        match self.state {
            ScoringJobState::Queued => {}
            ScoringJobState::RetryScheduled => {
                if claimed_at_unix_ms < self.next_attempt_at_unix_ms {
                    return Err(ScoringJobError::LeaseNotDue);
                }
            }
            _ => return Err(ScoringJobError::NotLeaseable),
        }

        self.attempt_count += 1;
        let fencing_token = u64::from(self.attempt_count);
        let lease = ScoringLease {
            worker_ref: worker_ref.to_owned(),
            lease_ref: lease_ref.to_owned(),
            fencing_token,
            expires_at_unix_ms,
        };
        self.state = ScoringJobState::Leased;
        self.next_attempt_at_unix_ms = 0;
        self.active_lease = Some(lease.clone());
        Ok(lease)
    }

    /// Record a retryable failed attempt using the current live fencing token.
    ///
    /// The job is scheduled for another attempt when budget remains. Exhausting
    /// the configured attempt budget moves it to quarantine instead of retrying
    /// indefinitely. A worker command observed at or after lease expiry is
    /// rejected even when the expiry-recovery sweep has not run yet.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError`] for invalid evidence, invalid retry timing,
    /// missing lease ownership, a stale fencing token, or expired lease authority.
    pub fn record_retryable_failure(
        &mut self,
        fencing_token: u64,
        cause_code: &str,
        failed_at_unix_ms: u64,
        retry_at_unix_ms: u64,
    ) -> Result<ScoringJobState, ScoringJobError> {
        let cause_code = required_reference(cause_code)?;
        require_timestamp(retry_at_unix_ms)?;
        if retry_at_unix_ms < failed_at_unix_ms {
            return Err(ScoringJobError::InvalidRetryWindow);
        }
        self.require_live_fencing_token(fencing_token, failed_at_unix_ms)?;
        self.last_failure_code = Some(cause_code.to_owned());
        self.active_lease = None;
        Ok(self.finish_failed_attempt(retry_at_unix_ms))
    }

    /// Record a permanent failed attempt and quarantine the job.
    ///
    /// The server-observed failure time must still fall within the worker lease.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError`] for invalid cause/timestamp evidence, missing
    /// lease ownership, a stale fencing token, or expired lease authority.
    pub fn record_permanent_failure(
        &mut self,
        fencing_token: u64,
        cause_code: &str,
        failed_at_unix_ms: u64,
    ) -> Result<(), ScoringJobError> {
        let cause_code = required_reference(cause_code)?;
        self.require_live_fencing_token(fencing_token, failed_at_unix_ms)?;
        self.last_failure_code = Some(cause_code.to_owned());
        self.active_lease = None;
        self.next_attempt_at_unix_ms = 0;
        self.state = ScoringJobState::Quarantined;
        Ok(())
    }

    /// Record one immutable successful scoring result.
    ///
    /// The first completion must be observed while the presenting worker lease
    /// is still live. Exact replay of the same accepted result/fence is
    /// idempotent; a different result or fence is rejected rather than rewriting
    /// historical scoring evidence.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError`] for invalid result/timestamp evidence,
    /// missing/stale/expired lease ownership, or conflicting completion evidence.
    pub fn record_success(
        &mut self,
        fencing_token: u64,
        scoring_result_ref: &str,
        completed_at_unix_ms: u64,
    ) -> Result<(), ScoringJobError> {
        let scoring_result_ref = required_reference(scoring_result_ref)?;
        require_timestamp(completed_at_unix_ms)?;
        if self.state == ScoringJobState::Completed {
            if self.result_ref.as_deref() != Some(scoring_result_ref)
                || self.completed_fencing_token != Some(fencing_token)
            {
                return Err(ScoringJobError::ConflictingCompletion);
            }
            return Ok(());
        }

        self.require_live_fencing_token(fencing_token, completed_at_unix_ms)?;
        self.result_ref = Some(scoring_result_ref.to_owned());
        self.completed_fencing_token = Some(fencing_token);
        self.active_lease = None;
        self.next_attempt_at_unix_ms = 0;
        self.state = ScoringJobState::Completed;
        Ok(())
    }

    /// Recover an expired lease into a due retry or quarantine.
    ///
    /// The failed attempt retains the typed `lease_expired` cause. Expiry never
    /// silently grants another worker ownership; a subsequent [`Self::claim`]
    /// must issue a new fencing token.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError`] when time is invalid, no lease is active, or
    /// the lease has not yet expired.
    pub fn expire_lease(
        &mut self,
        observed_at_unix_ms: u64,
    ) -> Result<ScoringJobState, ScoringJobError> {
        require_timestamp(observed_at_unix_ms)?;
        let lease = self.require_active_lease()?;
        if observed_at_unix_ms < lease.expires_at_unix_ms {
            return Err(ScoringJobError::LeaseStillActive);
        }
        self.last_failure_code = Some(LEASE_EXPIRED_CAUSE.to_owned());
        self.active_lease = None;
        Ok(self.finish_failed_attempt(observed_at_unix_ms))
    }

    /// Cancel queued, leased, or retry-scheduled work and invalidate any lease.
    ///
    /// Repeating cancellation is idempotent. Completed or quarantined evidence
    /// is terminal and cannot be rewritten as cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringJobError::TerminalState`] for completed or quarantined
    /// jobs.
    pub fn cancel(&mut self) -> Result<(), ScoringJobError> {
        match self.state {
            ScoringJobState::Cancelled => Ok(()),
            ScoringJobState::Completed | ScoringJobState::Quarantined => {
                Err(ScoringJobError::TerminalState)
            }
            ScoringJobState::Queued | ScoringJobState::Leased | ScoringJobState::RetryScheduled => {
                self.state = ScoringJobState::Cancelled;
                self.active_lease = None;
                self.next_attempt_at_unix_ms = 0;
                Ok(())
            }
        }
    }

    fn require_active_lease(&self) -> Result<&ScoringLease, ScoringJobError> {
        self.active_lease.as_ref().ok_or(ScoringJobError::NotLeased)
    }

    fn require_live_fencing_token(
        &self,
        fencing_token: u64,
        observed_at_unix_ms: u64,
    ) -> Result<(), ScoringJobError> {
        require_timestamp(observed_at_unix_ms)?;
        let lease = self.require_active_lease()?;
        if lease.fencing_token != fencing_token {
            return Err(ScoringJobError::StaleLease);
        }
        if observed_at_unix_ms >= lease.expires_at_unix_ms {
            return Err(ScoringJobError::LeaseExpired);
        }
        Ok(())
    }

    fn finish_failed_attempt(&mut self, retry_at_unix_ms: u64) -> ScoringJobState {
        if self.attempt_count >= self.max_attempts {
            self.state = ScoringJobState::Quarantined;
            self.next_attempt_at_unix_ms = 0;
        } else {
            self.state = ScoringJobState::RetryScheduled;
            self.next_attempt_at_unix_ms = retry_at_unix_ms;
        }
        self.state
    }
}

/// Fail-closed validation or lifecycle error for asynchronous scoring work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringJobError {
    /// A job, request, worker, lease, result, or cause reference is invalid.
    InvalidReference,
    /// The maximum automatic attempt count is zero.
    InvalidAttemptLimit,
    /// A server-authoritative timestamp is zero.
    InvalidTimestamp,
    /// A lease expiry is not later than its claim time.
    InvalidLeaseWindow,
    /// A retry instant precedes the failure instant that scheduled it.
    InvalidRetryWindow,
    /// A retry-scheduled job was claimed before its due time.
    LeaseNotDue,
    /// The current state cannot issue another worker lease.
    NotLeaseable,
    /// The job does not currently have a worker lease.
    NotLeased,
    /// A worker presented a fencing token from a different lease attempt.
    StaleLease,
    /// A worker command was observed after the presenting lease expired.
    LeaseExpired,
    /// Lease expiry was requested before the active lease expired.
    LeaseStillActive,
    /// Completed immutable evidence was replayed with a different result or token.
    ConflictingCompletion,
    /// Completed or quarantined evidence cannot be cancelled/re-written.
    TerminalState,
}

impl Display for ScoringJobError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "scoring job references must use their exact opaque non-numeric spelling"
            }
            Self::InvalidAttemptLimit => "scoring job maximum attempts must be greater than zero",
            Self::InvalidTimestamp => "scoring job timestamps must be greater than zero",
            Self::InvalidLeaseWindow => "scoring lease expiry must be later than claim time",
            Self::InvalidRetryWindow => "scoring retry time must not precede failure time",
            Self::LeaseNotDue => "scoring job retry is not due yet",
            Self::NotLeaseable => "scoring job is not available for a new lease",
            Self::NotLeased => "scoring job has no active lease",
            Self::StaleLease => "scoring job lease fencing token is stale",
            Self::LeaseExpired => "scoring job lease authority has expired",
            Self::LeaseStillActive => "scoring job lease has not expired",
            Self::ConflictingCompletion => {
                "scoring job already completed with different result evidence"
            }
            Self::TerminalState => "scoring job terminal state cannot accept this command",
        })
    }
}

impl Error for ScoringJobError {}

fn required_reference(reference: &str) -> Result<&str, ScoringJobError> {
    canonical_opaque_reference(reference).ok_or(ScoringJobError::InvalidReference)
}

const fn require_timestamp(timestamp: u64) -> Result<(), ScoringJobError> {
    if timestamp == 0 {
        Err(ScoringJobError::InvalidTimestamp)
    } else {
        Ok(())
    }
}
