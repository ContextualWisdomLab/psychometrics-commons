//! Atomic `PostgreSQL` composition for permanent scoring failure and outbox evidence.
//!
//! This module does not calculate psychometric quantities or invent a score. It
//! composes the existing fenced permanent-failure transition with the existing
//! durable transactional outbox inside one caller-owned transaction so a
//! quarantined scientific failure cannot commit while its integration evidence
//! is lost.

use crate::integration::IntegrationEvent;
use crate::postgres_integration::{enqueue_outbox_event, PersistenceDisposition, PersistenceError};
use crate::postgres_scoring_job::{
    record_permanent_scoring_failure, ScoringJobFailureDisposition, ScoringJobPersistenceError,
};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const SOURCE_REF: &str = "psychometrics_commons";

/// Durable dispositions produced by one atomic scoring-failure/outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringFailureOutboxPersistence {
    failure: ScoringJobFailureDisposition,
    outbox: PersistenceDisposition,
}

impl ScoringFailureOutboxPersistence {
    /// Return whether permanent failure was newly quarantined or exactly replayed.
    #[must_use]
    pub const fn failure(self) -> ScoringJobFailureDisposition {
        self.failure
    }

    /// Return whether immutable outbox evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.outbox
    }
}

/// Fail-closed error for atomic scoring failure and outbox persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringFailureOutboxError {
    /// The outbox envelope is not bound to this exact scoring-job failure boundary.
    InvalidFailureEnvelope,
    /// The fenced scoring-job failure transition failed validation or persistence.
    Failure(ScoringJobPersistenceError),
    /// The immutable integration outbox event failed validation or persistence.
    Outbox(PersistenceError),
}

impl Display for ScoringFailureOutboxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFailureEnvelope => {
                "scoring failure outbox must bind the exact job and failure time"
            }
            Self::Failure(_) => "scoring failure persistence failed",
            Self::Outbox(_) => "scoring failure outbox persistence failed",
        })
    }
}

impl Error for ScoringFailureOutboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidFailureEnvelope => None,
            Self::Failure(error) => Some(error),
            Self::Outbox(error) => Some(error),
        }
    }
}

/// Persist one fenced permanent scoring failure and one immutable outbox event atomically.
///
/// Before any write, the outbox event must be emitted by `psychometrics_commons`, identify the
/// exact scoring job as its subject, and carry the same server-authoritative failure time.
/// Event type, tenant, schema version, correlation, causation, payload digest, event identity,
/// and `cause_code` stay on the caller's versioned integration contract; this composition
/// does not add a schema that binds those fields. A new `event_ref` after an already-accepted
/// failure therefore inserts a second outbox row rather than rewriting historical quarantine
/// evidence.
///
/// The caller supplies and owns a `READ COMMITTED` transaction. After envelope validation, the
/// scoring-job transition runs first and the outbox insert runs in the same transaction. If either
/// stage fails, callers must roll the transaction back so no newly written quarantine state can
/// survive without its outbox evidence. Exact replay remains idempotent at both existing adapters,
/// and mixed exact dispositions can safely reconcile legacy partial state without mutating
/// historical evidence.
///
/// # Errors
///
/// Returns [`ScoringFailureOutboxError::InvalidFailureEnvelope`] before writes when the
/// integration event is bound to another source, job, or failure time,
/// [`ScoringFailureOutboxError::Failure`] when fenced scoring failure fails, or
/// [`ScoringFailureOutboxError::Outbox`] when the integration event cannot be persisted.
#[allow(clippy::too_many_arguments)]
pub fn record_permanent_scoring_failure_with_outbox(
    transaction: &mut Transaction<'_>,
    scoring_job_ref: &str,
    fencing_token: u64,
    cause_code: &str,
    failed_at_unix_ms: u64,
    failure_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ScoringFailureOutboxPersistence, ScoringFailureOutboxError> {
    validate_failure_envelope(scoring_job_ref, failed_at_unix_ms, failure_event)?;
    let failure = record_permanent_scoring_failure(
        transaction,
        scoring_job_ref,
        fencing_token,
        cause_code,
        failed_at_unix_ms,
    )
    .map_err(ScoringFailureOutboxError::Failure)?;
    let outbox = enqueue_outbox_event(transaction, failure_event, outbox_max_attempts)
        .map_err(ScoringFailureOutboxError::Outbox)?;

    Ok(ScoringFailureOutboxPersistence { failure, outbox })
}

fn validate_failure_envelope(
    scoring_job_ref: &str,
    failed_at_unix_ms: u64,
    failure_event: &IntegrationEvent,
) -> Result<(), ScoringFailureOutboxError> {
    if failure_event.source() != SOURCE_REF
        || failure_event.subject_ref() != scoring_job_ref
        || failure_event.occurred_at_unix_ms() != failed_at_unix_ms
    {
        return Err(ScoringFailureOutboxError::InvalidFailureEnvelope);
    }
    Ok(())
}
