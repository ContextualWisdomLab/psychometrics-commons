//! Atomic `PostgreSQL` composition for response snapshots and scoring dispatch.
//!
//! This adapter closes the crash boundary between freezing accepted response evidence and
//! durably scheduling its version-pinned scoring work. It composes existing immutable snapshot,
//! scoring-request, scoring-job, and transactional-outbox adapters in one caller-owned local
//! transaction. Psychometric arithmetic remains in `fast-mlsirm`.

use crate::integration::IntegrationEvent;
use crate::postgres_integration::PersistenceDisposition;
use crate::postgres_response_snapshot::{
    persist_response_snapshot, ResponseSnapshotPersistenceDisposition,
    ResponseSnapshotPersistenceError,
};
use crate::postgres_scoring_job::ScoringJobPersistenceDisposition;
use crate::postgres_scoring_request::{
    persist_scoring_dispatch, ScoringDispatchPersistence, ScoringDispatchPersistenceError,
    ScoringRequestPersistenceDisposition,
};
use crate::response::ResponseSnapshot;
use crate::scoring::ScoringRequest;
use crate::scoring_job::ScoringJob;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Durable dispositions produced by one response-snapshot/scoring-dispatch transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotScoringPersistence {
    response_snapshot: ResponseSnapshotPersistenceDisposition,
    dispatch: ScoringDispatchPersistence,
}

impl SnapshotScoringPersistence {
    /// Return whether immutable response-snapshot evidence was inserted or exactly replayed.
    #[must_use]
    pub const fn response_snapshot(self) -> ResponseSnapshotPersistenceDisposition {
        self.response_snapshot
    }

    /// Return whether immutable scoring-request evidence was inserted or exactly replayed.
    #[must_use]
    pub const fn scoring_request(self) -> ScoringRequestPersistenceDisposition {
        self.dispatch.scoring_request()
    }

    /// Return whether durable scoring-job evidence was inserted or exactly replayed.
    #[must_use]
    pub const fn scoring_job(self) -> ScoringJobPersistenceDisposition {
        self.dispatch.scoring_job()
    }

    /// Return whether immutable transactional-outbox evidence was inserted or exactly replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.dispatch.outbox()
    }
}

/// Fail-closed error for response-snapshot/scoring-dispatch persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum SnapshotScoringPersistenceError {
    /// The scoring request does not name the exact supplied snapshot and session.
    MismatchedSnapshotBinding,
    /// Immutable response-snapshot persistence failed.
    Snapshot(ResponseSnapshotPersistenceError),
    /// Scoring request/job/outbox persistence failed.
    Dispatch(ScoringDispatchPersistenceError),
}

impl Display for SnapshotScoringPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MismatchedSnapshotBinding => {
                "scoring request must bind the exact response snapshot and session"
            }
            Self::Snapshot(_) => "response snapshot persistence failed before scoring dispatch",
            Self::Dispatch(_) => "scoring dispatch persistence failed after response snapshot",
        })
    }
}

impl Error for SnapshotScoringPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MismatchedSnapshotBinding => None,
            Self::Snapshot(error) => Some(error),
            Self::Dispatch(error) => Some(error),
        }
    }
}

/// Persist one frozen response snapshot and its scoring request/job/outbox as one transaction.
///
/// The request must name the supplied snapshot identity and the same session before any write is
/// attempted. The caller owns the `READ COMMITTED` transaction and final commit/rollback decision.
/// If snapshot persistence or any dispatch stage fails, the caller must roll back so a snapshot is
/// never acknowledged without its local scoring work, and newly scheduled scoring work is never
/// detached from its immutable response evidence. Exact replay remains idempotent at every
/// composed adapter.
///
/// This function does not define the external scoring event schema and does not execute scoring;
/// it only removes the local durability gap before asynchronous dispatch.
///
/// # Errors
///
/// Returns [`SnapshotScoringPersistenceError::MismatchedSnapshotBinding`] before writes for a
/// snapshot/session mismatch, or preserves typed snapshot/dispatch persistence failures.
pub fn persist_response_snapshot_and_scoring_dispatch(
    transaction: &mut Transaction<'_>,
    snapshot: &ResponseSnapshot,
    request: &ScoringRequest,
    job: &ScoringJob,
    dispatch_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<SnapshotScoringPersistence, SnapshotScoringPersistenceError> {
    if snapshot.snapshot_ref() != Some(request.response_snapshot_ref())
        || snapshot.session_ref() != request.session_ref()
    {
        return Err(SnapshotScoringPersistenceError::MismatchedSnapshotBinding);
    }

    let response_snapshot = persist_response_snapshot(transaction, snapshot)
        .map_err(SnapshotScoringPersistenceError::Snapshot)?;
    let dispatch = persist_scoring_dispatch(
        transaction,
        request,
        job,
        dispatch_event,
        outbox_max_attempts,
    )
    .map_err(SnapshotScoringPersistenceError::Dispatch)?;

    Ok(SnapshotScoringPersistence {
        response_snapshot,
        dispatch,
    })
}
