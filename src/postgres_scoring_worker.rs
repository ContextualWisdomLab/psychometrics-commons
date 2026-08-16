//! Atomic `PostgreSQL` scoring-worker commit that reuses a stable terminal event identity.
//!
//! This module does not calculate psychometric quantities or invent an event schema.
//! It binds the caller's outbox envelope to the stable job-plus-result or job-plus-cause
//! identity, then composes the existing completion or permanent-failure helper inside the
//! caller-owned transaction. A minted `event_ref` is rejected before any write.

use crate::integration::IntegrationEvent;
use crate::postgres_result_snapshot::{
    persist_result_snapshot, ResultSnapshotPersistenceDisposition, ResultSnapshotPersistenceError,
};
use crate::postgres_scoring_completion::{
    record_successful_scoring_completion_with_outbox, ScoringCompletionOutboxError,
    ScoringCompletionOutboxPersistence,
};
use crate::postgres_scoring_failure::{
    record_permanent_scoring_failure_with_outbox, ScoringFailureOutboxError,
    ScoringFailureOutboxPersistence,
};
use crate::postgres_scoring_request::{load_scoring_request, ScoringRequestPersistenceError};
use crate::result::ResultSnapshotInput;
use crate::scoring_worker::{
    plan_scoring_worker_attempt, plan_scoring_worker_result_attempt, require_stable_terminal_event,
    ScoringTerminalIdentity, ScoringWorkerAttempt, ScoringWorkerEngine, ScoringWorkerEngineOutcome,
    ScoringWorkerEnvelope, ScoringWorkerError, ScoringWorkerResultEngine,
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

/// Durable dispositions produced by one request-load plus result-snapshot worker attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringWorkerSnapshotPersistence {
    terminal: ScoringWorkerPersistence,
    snapshot: Option<ResultSnapshotPersistenceDisposition>,
}

impl ScoringWorkerSnapshotPersistence {
    /// Return the terminal job and outbox disposition.
    #[must_use]
    pub const fn terminal(self) -> ScoringWorkerPersistence {
        self.terminal
    }

    /// Return whether the immutable result snapshot was inserted or exactly replayed.
    #[must_use]
    pub const fn snapshot(self) -> Option<ResultSnapshotPersistenceDisposition> {
        self.snapshot
    }
}

/// Fail-closed error for a scoring-worker terminal commit.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringWorkerCommitError {
    /// The supplied event identity is not the stable job and outcome key.
    Identity(ScoringWorkerError),
    /// The engine or planner could not produce a typed terminal attempt.
    Planning(ScoringWorkerError),
    /// The persisted scoring request could not be reconstructed.
    Request(ScoringRequestPersistenceError),
    /// The scoring job named a request that is not stored after restart.
    MissingRequest,
    /// Immutable result-snapshot persistence failed.
    Snapshot(ResultSnapshotPersistenceError),
    /// Fenced successful completion or its outbox evidence failed.
    Completion(ScoringCompletionOutboxError),
    /// Fenced permanent failure or its outbox evidence failed.
    Failure(ScoringFailureOutboxError),
}

impl Display for ScoringWorkerCommitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Identity(_) => {
                "scoring worker must reuse the stable job and outcome event identity"
            }
            Self::Planning(_) => {
                "scoring worker could not plan a terminal attempt; keep the job leased and retry after a typed engine outcome"
            }
            Self::Request(_) => {
                "scoring worker could not reconstruct the persisted scoring request; keep the job leased"
            }
            Self::MissingRequest => {
                "reload the persisted scoring request before completing the job; do not invent a score"
            }
            Self::Snapshot(_) => {
                "scoring worker could not persist the immutable result snapshot; keep the job leased"
            }
            Self::Completion(_) => "scoring worker completion persistence failed",
            Self::Failure(_) => "scoring worker failure persistence failed",
        })
    }
}

impl Error for ScoringWorkerCommitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Identity(error) | Self::Planning(error) => Some(error),
            Self::Request(error) => Some(error),
            Self::MissingRequest => None,
            Self::Snapshot(error) => Some(error),
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
/// fenced successful completion fails, or [`ScoringWorkerCommitError::Failure`] when
/// permanent failure fails.
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

/// Run one fenced scoring-worker attempt through a test-double or later live engine.
///
/// The engine is asked first. The planner then binds the stable job-plus-result or
/// job-plus-cause `event_ref` so this function cannot mint a second terminal identity
/// after an accepted write. Event type, tenant, schema, correlation, causation, and
/// payload digest stay on the caller contract. Live `fast-mlsirm` execution remains a
/// later adapter behind [`ScoringWorkerEngine`].
///
/// # Errors
///
/// Returns [`ScoringWorkerCommitError::Planning`] when the engine or planner cannot
/// produce a typed terminal attempt, [`ScoringWorkerCommitError::Completion`] when
/// successful completion fails, or [`ScoringWorkerCommitError::Failure`] when
/// permanent failure fails.
pub fn run_scoring_worker_attempt(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    scoring_request_ref: &str,
    engine: &impl ScoringWorkerEngine,
    envelope: ScoringWorkerEnvelope<'_>,
    outbox_max_attempts: usize,
) -> Result<ScoringWorkerPersistence, ScoringWorkerCommitError> {
    let attempt =
        plan_scoring_worker_attempt(scoring_job_ref, scoring_request_ref, engine, envelope)
            .map_err(ScoringWorkerCommitError::Planning)?;
    commit_planned_scoring_worker_attempt(
        transaction,
        scoring_job_ref,
        fencing_token,
        &attempt,
        outbox_max_attempts,
    )
}

fn commit_planned_scoring_worker_attempt(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    attempt: &ScoringWorkerAttempt,
    outbox_max_attempts: usize,
) -> Result<ScoringWorkerPersistence, ScoringWorkerCommitError> {
    let outcome = match attempt.outcome() {
        ScoringWorkerEngineOutcome::Completed { result_ref } => {
            ScoringWorkerOutcome::Completed { result_ref }
        }
        ScoringWorkerEngineOutcome::Failed { cause_code } => {
            ScoringWorkerOutcome::Failed { cause_code }
        }
    };
    commit_scoring_worker_outcome(
        transaction,
        scoring_job_ref,
        fencing_token,
        outcome,
        attempt.event().occurred_at_unix_ms(),
        attempt.event(),
        outbox_max_attempts,
    )
}

/// Load the persisted scoring request, persist the result snapshot, then commit.
///
/// After restart, the worker reconstructs the version pin from `scoring_request`
/// and asks a request-bound engine. A completed result and its product snapshot
/// commit in the same caller-owned transaction as the fenced job and outbox
/// evidence. A missing request, planner failure, or snapshot conflict leaves the
/// leased job untouched when the caller rolls back. Live `fast-mlsirm` execution
/// remains a later adapter behind [`ScoringWorkerResultEngine`].
///
/// # Errors
///
/// Returns [`ScoringWorkerCommitError::MissingRequest`] when the pin is absent,
/// [`ScoringWorkerCommitError::Request`] when reconstruction fails,
/// [`ScoringWorkerCommitError::Planning`] when the engine or planner cannot
/// produce a typed attempt, [`ScoringWorkerCommitError::Snapshot`] when the
/// immutable snapshot cannot persist, or the existing completion/failure error.
#[allow(clippy::too_many_arguments)]
pub fn run_scoring_worker_attempt_with_result_snapshot(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    scoring_request_ref: &str,
    engine: &impl ScoringWorkerResultEngine,
    snapshot_input: ResultSnapshotInput<'_>,
    envelope: ScoringWorkerEnvelope<'_>,
    outbox_max_attempts: usize,
) -> Result<ScoringWorkerSnapshotPersistence, ScoringWorkerCommitError> {
    let request = load_scoring_request(transaction, scoring_request_ref)
        .map_err(ScoringWorkerCommitError::Request)?
        .ok_or(ScoringWorkerCommitError::MissingRequest)?;
    let attempt = plan_scoring_worker_result_attempt(
        scoring_job_ref,
        &request,
        engine,
        snapshot_input,
        envelope,
    )
    .map_err(ScoringWorkerCommitError::Planning)?;
    let snapshot = match attempt.snapshot() {
        Some(snapshot) => Some(
            persist_result_snapshot(transaction, snapshot)
                .map_err(ScoringWorkerCommitError::Snapshot)?,
        ),
        None => None,
    };
    let terminal = commit_planned_scoring_worker_attempt(
        transaction,
        scoring_job_ref,
        fencing_token,
        &ScoringWorkerAttempt::from_planned(attempt.outcome().clone(), attempt.event().clone()),
        outbox_max_attempts,
    )?;
    Ok(ScoringWorkerSnapshotPersistence { terminal, snapshot })
}
