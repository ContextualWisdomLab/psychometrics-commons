//! Atomic `PostgreSQL` scoring-worker commit that reuses a stable terminal event identity.
//!
//! This module does not calculate psychometric quantities or invent an event schema.
//! It binds the caller's outbox envelope to the stable job-plus-result or job-plus-cause
//! identity, then composes the existing completion or permanent-failure helper inside the
//! caller-owned transaction. A minted `event_ref` is rejected before any write.

use crate::integration::IntegrationEvent;
use crate::postgres_scoring_completion::{
    record_successful_scoring_completion_with_outbox, ScoringCompletionOutboxError,
    ScoringCompletionOutboxPersistence,
};
use crate::postgres_scoring_failure::{
    record_permanent_scoring_failure_with_outbox, ScoringFailureOutboxError,
    ScoringFailureOutboxPersistence,
};
use crate::postgres_scoring_job::{record_retryable_scoring_failure, ScoringJobPersistenceError};
use crate::scoring_job::ScoringJobState;
use crate::scoring_worker::{
    plan_scoring_worker_attempt, require_stable_terminal_event, ScoringEngineAttempt,
    ScoringTerminalIdentity, ScoringWorkerError, ScoringWorkerPlan,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Terminal scoring outcome presented by one fenced worker attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringWorkerOutcome<'a> {
    /// The worker accepted one immutable scoring result.
    Completed {
        /// Opaque identity of the accepted scoring result.
        result_ref: &'a str,
    },
    /// The worker recorded one permanent scientific failure cause.
    Failed {
        /// Typed cause retained for quarantine and exact replay.
        cause_code: &'a str,
    },
}

/// Durable dispositions produced by one scoring-worker terminal commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringWorkerPersistence {
    /// Successful completion and its bound outbox evidence.
    Completed(ScoringCompletionOutboxPersistence),
    /// Permanent failure and its bound outbox evidence.
    Failed(ScoringFailureOutboxPersistence),
}

/// Fail-closed error for a scoring-worker terminal commit.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringWorkerCommitError {
    /// The supplied event identity is not the stable job and outcome key.
    Identity(ScoringWorkerError),
    /// Fenced successful completion or its outbox evidence failed.
    Completion(ScoringCompletionOutboxError),
    /// Fenced permanent failure or its outbox evidence failed.
    Failure(ScoringFailureOutboxError),
}

impl Display for ScoringWorkerCommitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Identity(_) => "scoring worker terminal identity is invalid",
            Self::Completion(_) => "scoring worker completion persistence failed",
            Self::Failure(_) => "scoring worker failure persistence failed",
        })
    }
}

impl Error for ScoringWorkerCommitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Completion(error) => Some(error),
            Self::Failure(error) => Some(error),
        }
    }
}

/// Persist one fenced terminal scoring outcome with a stable outbox event identity.
///
/// The outbox `event_ref` must be the stable identity for this job and result, or this
/// job and cause. Event type, tenant, schema version, correlation, causation, payload
/// digest, and `cause_code` / result binding stay on the caller's versioned integration
/// contract. The caller supplies and owns a `READ COMMITTED` transaction and must roll
/// it back when this function returns an error.
///
/// # Errors
///
/// Returns [`ScoringWorkerCommitError::Identity`] before writes when the event identity
/// is not the stable job/outcome key, [`ScoringWorkerCommitError::Completion`] when
/// successful completion fails, or [`ScoringWorkerCommitError::Failure`] when permanent
/// failure fails.
#[allow(clippy::too_many_arguments)]
pub fn commit_scoring_worker_outcome(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    outcome: ScoringWorkerOutcome<'_>,
    occurred_at_unix_ms: u64,
    terminal_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ScoringWorkerPersistence, ScoringWorkerCommitError> {
    match outcome {
        ScoringWorkerOutcome::Completed { result_ref } => {
            require_stable_terminal_event(
                scoring_job_ref,
                ScoringTerminalIdentity::Result(result_ref),
                terminal_event,
            )
            .map_err(ScoringWorkerCommitError::Identity)?;
            record_successful_scoring_completion_with_outbox(
                transaction,
                scoring_job_ref,
                fencing_token,
                result_ref,
                occurred_at_unix_ms,
                terminal_event,
                outbox_max_attempts,
            )
            .map(ScoringWorkerPersistence::Completed)
            .map_err(ScoringWorkerCommitError::Completion)
        }
        ScoringWorkerOutcome::Failed { cause_code } => {
            require_stable_terminal_event(
                scoring_job_ref,
                ScoringTerminalIdentity::Cause(cause_code),
                terminal_event,
            )
            .map_err(ScoringWorkerCommitError::Identity)?;
            record_permanent_scoring_failure_with_outbox(
                transaction,
                scoring_job_ref,
                fencing_token,
                cause_code,
                occurred_at_unix_ms,
                terminal_event,
                outbox_max_attempts,
            )
            .map(ScoringWorkerPersistence::Failed)
            .map_err(ScoringWorkerCommitError::Failure)
        }
    }
}

/// Durable dispositions produced by one fenced scoring-worker attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringWorkerAttemptPersistence {
    /// A terminal completion or permanent failure and its bound outbox evidence.
    Terminal(ScoringWorkerPersistence),
    /// A retryable engine failure recorded without a terminal outbox row.
    Retryable(ScoringJobState),
}

/// Fail-closed error for one fenced scoring-worker attempt.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringWorkerAttemptError {
    /// The engine outcome could not be bound to a stable terminal identity.
    Identity(ScoringWorkerError),
    /// Fenced successful completion or its outbox evidence failed.
    Completion(ScoringCompletionOutboxError),
    /// Fenced permanent failure or its outbox evidence failed.
    Failure(ScoringFailureOutboxError),
    /// Retryable engine failure could not be recorded on the current lease.
    Retry(ScoringJobPersistenceError),
}

impl Display for ScoringWorkerAttemptError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Identity(_) => {
                "bind the stable job and outcome event identity before the terminal write"
            }
            Self::Completion(_) => "scoring worker completion persistence failed",
            Self::Failure(_) => "scoring worker failure persistence failed",
            Self::Retry(_) => "record the retryable engine failure without a terminal outbox row",
        })
    }
}

impl Error for ScoringWorkerAttemptError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::Completion(error) => Some(error),
            Self::Failure(error) => Some(error),
            Self::Retry(error) => Some(error),
        }
    }
}

/// Run one fenced scoring-worker attempt from a replaceable engine outcome.
///
/// Completed and permanently failed outcomes rewrite a minted envelope `event_ref` to the
/// stable job-plus-result or job-plus-cause identity, then reuse
/// [`commit_scoring_worker_outcome`]. Retryable outcomes call the existing retry helper and
/// insert no terminal outbox row. Event type, tenant, schema version, correlation,
/// causation, payload digest, and `cause_code` stay on the caller contract.
///
/// The caller supplies and owns a `READ COMMITTED` transaction and must roll it back when
/// this function returns an error.
///
/// # Errors
///
/// Returns [`ScoringWorkerAttemptError::Identity`] when the engine outcome cannot be bound,
/// [`ScoringWorkerAttemptError::Completion`] or [`ScoringWorkerAttemptError::Failure`] when
/// a terminal write fails, or [`ScoringWorkerAttemptError::Retry`] when retryable failure
/// cannot be recorded.
#[allow(clippy::too_many_arguments)]
pub fn run_scoring_worker_attempt(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    attempt: ScoringEngineAttempt<'_>,
    envelope: &IntegrationEvent,
    occurred_at_unix_ms: u64,
    retry_at_unix_ms: u64,
    outbox_max_attempts: usize,
) -> Result<ScoringWorkerAttemptPersistence, ScoringWorkerAttemptError> {
    let plan = plan_scoring_worker_attempt(scoring_job_ref, attempt, envelope)
        .map_err(ScoringWorkerAttemptError::Identity)?;
    match plan {
        ScoringWorkerPlan::Complete { result_ref, event } => commit_scoring_worker_outcome(
            transaction,
            scoring_job_ref,
            fencing_token,
            ScoringWorkerOutcome::Completed { result_ref },
            occurred_at_unix_ms,
            &event,
            outbox_max_attempts,
        )
        .map(ScoringWorkerAttemptPersistence::Terminal)
        .map_err(map_commit_error),
        ScoringWorkerPlan::FailPermanently { cause_code, event } => commit_scoring_worker_outcome(
            transaction,
            scoring_job_ref,
            fencing_token,
            ScoringWorkerOutcome::Failed { cause_code },
            occurred_at_unix_ms,
            &event,
            outbox_max_attempts,
        )
        .map(ScoringWorkerAttemptPersistence::Terminal)
        .map_err(map_commit_error),
        ScoringWorkerPlan::Retry { cause_code } => record_retryable_scoring_failure(
            transaction,
            scoring_job_ref,
            fencing_token,
            cause_code,
            occurred_at_unix_ms,
            retry_at_unix_ms,
        )
        .map(ScoringWorkerAttemptPersistence::Retryable)
        .map_err(ScoringWorkerAttemptError::Retry),
    }
}

fn map_commit_error(error: ScoringWorkerCommitError) -> ScoringWorkerAttemptError {
    match error {
        ScoringWorkerCommitError::Identity(error) => ScoringWorkerAttemptError::Identity(error),
        ScoringWorkerCommitError::Completion(error) => ScoringWorkerAttemptError::Completion(error),
        ScoringWorkerCommitError::Failure(error) => ScoringWorkerAttemptError::Failure(error),
    }
}

#[cfg(test)]
mod attempt_error_mapping_tests {
    use super::{map_commit_error, ScoringWorkerAttemptError, ScoringWorkerCommitError};
    use crate::postgres_scoring_completion::ScoringCompletionOutboxError;
    use crate::postgres_scoring_failure::ScoringFailureOutboxError;
    use crate::scoring_worker::ScoringWorkerError;

    #[test]
    fn commit_errors_keep_their_typed_attempt_sources() {
        assert!(matches!(
            map_commit_error(ScoringWorkerCommitError::Identity(
                ScoringWorkerError::UnstableEventRef
            )),
            ScoringWorkerAttemptError::Identity(ScoringWorkerError::UnstableEventRef)
        ));
        assert!(matches!(
            map_commit_error(ScoringWorkerCommitError::Completion(
                ScoringCompletionOutboxError::InvalidCompletionEnvelope
            )),
            ScoringWorkerAttemptError::Completion(
                ScoringCompletionOutboxError::InvalidCompletionEnvelope
            )
        ));
        assert!(matches!(
            map_commit_error(ScoringWorkerCommitError::Failure(
                ScoringFailureOutboxError::InvalidFailureEnvelope
            )),
            ScoringWorkerAttemptError::Failure(ScoringFailureOutboxError::InvalidFailureEnvelope)
        ));
    }
}
