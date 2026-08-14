//! Atomic `PostgreSQL` composition for scoring completion and integration outbox evidence.
//!
//! This module does not calculate psychometric quantities or define an external event schema.
//! It composes the existing fenced scoring-job completion transition with the existing durable
//! transactional outbox inside one caller-owned transaction so a successful result transition
//! cannot commit while its integration evidence is lost.

use crate::integration::IntegrationEvent;
use crate::postgres_integration::{enqueue_outbox_event, PersistenceDisposition, PersistenceError};
use crate::postgres_scoring_job::{
    record_successful_scoring_completion, ScoringJobCompletionDisposition,
    ScoringJobPersistenceError,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Durable dispositions produced by one atomic scoring-completion/outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringCompletionOutboxPersistence {
    completion: ScoringJobCompletionDisposition,
    outbox: PersistenceDisposition,
}

impl ScoringCompletionOutboxPersistence {
    /// Return whether scoring completion was newly committed or exactly replayed.
    #[must_use]
    pub const fn completion(self) -> ScoringJobCompletionDisposition {
        self.completion
    }

    /// Return whether immutable outbox evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.outbox
    }
}

/// Fail-closed error for atomic scoring completion and outbox persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringCompletionOutboxError {
    /// The fenced scoring-job completion transition failed validation or persistence.
    Completion(ScoringJobPersistenceError),
    /// The immutable integration outbox event failed validation or persistence.
    Outbox(PersistenceError),
}

impl Display for ScoringCompletionOutboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Completion(_) => "scoring completion persistence failed",
            Self::Outbox(_) => "scoring completion outbox persistence failed",
        })
    }
}

impl Error for ScoringCompletionOutboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Completion(error) => Some(error),
            Self::Outbox(error) => Some(error),
        }
    }
}

/// Persist one fenced successful scoring completion and one immutable outbox event atomically.
///
/// The caller supplies and owns a `READ COMMITTED` transaction. The scoring-job transition runs
/// first; the validated outbox insert then runs in the same transaction. If either stage fails,
/// callers must roll the transaction back so no newly written completion state can survive without
/// its outbox evidence. Exact replay remains idempotent at both existing adapters, and mixed exact
/// dispositions can safely reconcile legacy partial state without mutating historical evidence.
///
/// Event type, tenant, subject, correlation, causation, and payload semantics remain the caller's
/// versioned integration-contract responsibility. This composition guarantees transactional
/// durability, not semantic invention of a new external event contract.
///
/// # Errors
///
/// Returns [`ScoringCompletionOutboxError::Completion`] when fenced scoring completion fails, or
/// [`ScoringCompletionOutboxError::Outbox`] when the integration event cannot be persisted.
#[allow(clippy::too_many_arguments)]
pub fn record_successful_scoring_completion_with_outbox(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    scoring_result_ref: &str,
    completed_at_unix_ms: u64,
    completion_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ScoringCompletionOutboxPersistence, ScoringCompletionOutboxError> {
    let completion = record_successful_scoring_completion(
        transaction,
        scoring_job_ref,
        fencing_token,
        scoring_result_ref,
        completed_at_unix_ms,
    )
    .map_err(ScoringCompletionOutboxError::Completion)?;
    let outbox = enqueue_outbox_event(transaction, completion_event, outbox_max_attempts)
        .map_err(ScoringCompletionOutboxError::Outbox)?;

    Ok(ScoringCompletionOutboxPersistence { completion, outbox })
}