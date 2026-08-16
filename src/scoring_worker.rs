//! Scoring-worker terminal identity that keeps outbox evidence replay-safe.
//!
//! This module does not calculate psychometric quantities or call `fast-mlsirm`.
//! It binds one durable outbox event identity to the exact scoring job and the
//! accepted result or permanent cause so a crashed worker can reconcile without
//! minting a second event after an already-accepted terminal write.

use crate::integration::IntegrationEvent;
use crate::reference::normalized_reference;
use crate::result::{ResultSnapshot, ResultSnapshotError, ResultSnapshotInput};
use crate::scoring::{ScoringRequest, ScoringResult};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Identity that must produce one stable outbox event for a terminal scoring outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringTerminalIdentity<'a> {
    /// Successful completion bound to one immutable result identity.
    Result(&'a str),
    /// Permanent scientific failure bound to one typed cause.
    Cause(&'a str),
}

/// Fail-closed error for scoring-worker terminal identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringWorkerError {
    /// A job, result, or cause reference was blank or numeric-like.
    InvalidReference,
    /// The supplied outbox event used a different identity than the stable job/outcome key.
    UnstableEventRef,
    /// Caller envelope fields failed integration-event validation.
    InvalidEnvelope,
    /// The engine result is bound to a different scoring request than the loaded pin.
    MismatchedScoringResult,
    /// The product result snapshot cannot be built from the loaded request and engine result.
    InvalidResultSnapshot,
}

impl Display for ScoringWorkerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "scoring worker identities must be opaque non-numeric values",
            Self::UnstableEventRef => {
                "scoring worker must reuse the stable job and outcome event identity"
            }
            Self::InvalidEnvelope => {
                "scoring worker envelope fields must be valid integration evidence"
            }
            Self::MismatchedScoringResult => {
                "scoring worker must persist a result bound to the loaded scoring request"
            }
            Self::InvalidResultSnapshot => {
                "scoring worker must persist a valid immutable result snapshot before completing the job"
            }
        })
    }
}

impl Error for ScoringWorkerError {}

/// Return the stable outbox event identity for one terminal scoring outcome.
///
/// The same job and result, or the same job and cause, always produce the same
/// `event_ref`. Length prefixes keep references that contain `:` from colliding.
/// A later worker attempt must reuse that identity instead of minting a new event
/// after the terminal write was already accepted.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job, result, or cause
/// is blank or numeric-like.
pub fn scoring_terminal_event_ref(
    scoring_job_ref: &str,
    identity: ScoringTerminalIdentity<'_>,
) -> Result<String, ScoringWorkerError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let (kind, outcome_ref) = match identity {
        ScoringTerminalIdentity::Result(result_ref) => ("result", required_reference(result_ref)?),
        ScoringTerminalIdentity::Cause(cause_code) => ("cause", required_reference(cause_code)?),
    };
    Ok(format!(
        "scoring_terminal:{kind}:{}:{scoring_job_ref}:{}:{outcome_ref}",
        scoring_job_ref.len(),
        outcome_ref.len()
    ))
}

/// Reject an outbox envelope that does not use the stable job and outcome identity.
///
/// Call this before composing a terminal scoring write so a retry cannot insert a
/// second outbox row for the same accepted result or cause.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job or outcome identity
/// is invalid, or [`ScoringWorkerError::UnstableEventRef`] when `event` carries a
/// different `event_ref`.
pub fn require_stable_terminal_event(
    scoring_job_ref: &str,
    identity: ScoringTerminalIdentity<'_>,
    event: &IntegrationEvent,
) -> Result<(), ScoringWorkerError> {
    let expected = scoring_terminal_event_ref(scoring_job_ref, identity)?;
    if event.event_ref() != expected {
        return Err(ScoringWorkerError::UnstableEventRef);
    }
    Ok(())
}

fn required_reference(reference: &str) -> Result<&str, ScoringWorkerError> {
    normalized_reference(reference).ok_or(ScoringWorkerError::InvalidReference)
}

/// Terminal outcome returned by one scoring-engine attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoringWorkerEngineOutcome {
    /// The engine accepted one immutable scoring-result identity.
    Completed {
        /// Opaque identity of the accepted scoring result.
        result_ref: String,
    },
    /// The engine recorded one permanent scientific failure cause.
    Failed {
        /// Typed cause retained for quarantine and exact replay.
        cause_code: String,
    },
}

/// Scoring engine used by one fenced worker attempt.
///
/// Implementations must not persist product state or mint an outbox identity.
/// A later live `fast-mlsirm` adapter can replace a test double without changing
/// the worker's stable event-binding contract.
pub trait ScoringWorkerEngine {
    /// Score one claimed job and return a terminal result or permanent cause.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringWorkerError`] when the engine cannot produce a typed
    /// terminal outcome for this claimed job and request.
    fn score_claimed_job(
        &self,
        scoring_job_ref: &str,
        scoring_request_ref: &str,
    ) -> Result<ScoringWorkerEngineOutcome, ScoringWorkerError>;
}

/// Caller-owned integration envelope fields. The worker binds `event_ref`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringWorkerEnvelope<'a> {
    /// Versioned domain event type supplied by the caller contract.
    pub event_type: &'a str,
    /// Payload schema version supplied by the caller contract.
    pub schema_version: &'a str,
    /// Emitting bounded-context source identifier.
    pub source: &'a str,
    /// Tenant whose scoring job emitted this terminal evidence.
    pub tenant_ref: &'a str,
    /// Server-authoritative occurrence time shared with the job transition.
    pub occurred_at_unix_ms: u64,
    /// Request or workflow correlation reference.
    pub correlation_ref: &'a str,
    /// Optional causation reference, typically the scoring request.
    pub causation_ref: Option<&'a str>,
    /// Canonical SHA-256 payload digest.
    pub payload_digest: &'a str,
}

/// Planned terminal write: engine outcome plus the stable outbox envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoringWorkerAttempt {
    outcome: ScoringWorkerEngineOutcome,
    event: IntegrationEvent,
}

impl ScoringWorkerAttempt {
    /// Bind a planned engine outcome to its already-stable outbox envelope.
    #[must_use]
    pub(crate) fn from_planned(
        outcome: ScoringWorkerEngineOutcome,
        event: IntegrationEvent,
    ) -> Self {
        Self { outcome, event }
    }

    /// Return the engine outcome that must be committed.
    #[must_use]
    pub const fn outcome(&self) -> &ScoringWorkerEngineOutcome {
        &self.outcome
    }

    /// Return the outbox envelope bound to the stable job and outcome identity.
    #[must_use]
    pub const fn event(&self) -> &IntegrationEvent {
        &self.event
    }
}

/// Ask the engine, then bind the stable job-plus-outcome outbox identity.
///
/// The caller still owns event type, tenant, schema, correlation, causation, and
/// payload digest. `ScoringWorkerEnvelope` has no `event_ref`; this planner binds
/// the stable identity for the job plus accepted result or permanent cause.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job or request is
/// invalid, the engine's own identity error, or
/// [`ScoringWorkerError::InvalidEnvelope`] when caller envelope fields cannot
/// form an integration event.
pub fn plan_scoring_worker_attempt(
    scoring_job_ref: &str,
    scoring_request_ref: &str,
    engine: &impl ScoringWorkerEngine,
    envelope: ScoringWorkerEnvelope<'_>,
) -> Result<ScoringWorkerAttempt, ScoringWorkerError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    let scoring_request_ref = required_reference(scoring_request_ref)?;
    require_valid_worker_envelope(scoring_job_ref, envelope)?;
    let outcome = engine.score_claimed_job(scoring_job_ref, scoring_request_ref)?;
    let identity = match &outcome {
        ScoringWorkerEngineOutcome::Completed { result_ref } => {
            ScoringTerminalIdentity::Result(result_ref)
        }
        ScoringWorkerEngineOutcome::Failed { cause_code } => {
            ScoringTerminalIdentity::Cause(cause_code)
        }
    };
    let event_ref = scoring_terminal_event_ref(scoring_job_ref, identity)?;
    let event = IntegrationEvent::new(
        &event_ref,
        envelope.event_type,
        envelope.schema_version,
        envelope.source,
        envelope.tenant_ref,
        scoring_job_ref,
        envelope.occurred_at_unix_ms,
        envelope.correlation_ref,
        envelope.causation_ref,
        envelope.payload_digest,
    )
    .map_err(|_| ScoringWorkerError::InvalidEnvelope)?;
    Ok(ScoringWorkerAttempt { outcome, event })
}

const ENVELOPE_PROBE_EVENT_REF: &str = "scoring_worker_envelope_probe";

fn require_valid_worker_envelope(
    scoring_job_ref: &str,
    envelope: ScoringWorkerEnvelope<'_>,
) -> Result<(), ScoringWorkerError> {
    IntegrationEvent::new(
        ENVELOPE_PROBE_EVENT_REF,
        envelope.event_type,
        envelope.schema_version,
        envelope.source,
        envelope.tenant_ref,
        scoring_job_ref,
        envelope.occurred_at_unix_ms,
        envelope.correlation_ref,
        envelope.causation_ref,
        envelope.payload_digest,
    )
    .map(|_| ())
    .map_err(|_| ScoringWorkerError::InvalidEnvelope)
}

/// Terminal or retryable outcome returned by one request-bound scoring-engine attempt.
#[derive(Clone, Debug, PartialEq)]
pub enum ScoringWorkerResultOutcome {
    /// The engine accepted one immutable scoring result that can become a snapshot.
    Completed {
        /// Typed engine result bound to the loaded scoring request.
        result: Box<ScoringResult>,
    },
    /// The engine recorded one permanent scientific failure cause.
    Failed {
        /// Typed cause retained for quarantine and exact replay.
        cause_code: String,
    },
    /// The engine could not finish; a later attempt may succeed.
    ///
    /// The worker must schedule a retry, write no terminal outbox row, and must
    /// not invent a score.
    Retryable {
        /// Typed transport or engine-outage cause retained for the next attempt.
        cause_code: String,
    },
}

/// Scoring engine that receives the reconstructed version-pinned request.
///
/// Implementations must not persist product state or mint an outbox identity.
pub trait ScoringWorkerResultEngine {
    /// Score one claimed job using the reconstructed scoring request.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringWorkerError`] when the engine cannot produce a typed
    /// terminal or retryable outcome for this claimed job and request.
    fn score_claimed_request(
        &self,
        scoring_job_ref: &str,
        request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError>;
}

/// Planned request-bound write: a terminal snapshot/outbox commit or a retry.
#[derive(Clone, Debug, PartialEq)]
pub enum ScoringWorkerResultPlan {
    /// Persist the snapshot (when present), fenced job, and stable outbox event.
    Terminal(Box<ScoringWorkerResultAttempt>),
    /// Record a retryable outage without a terminal event or invented score.
    Retryable {
        /// Typed transport or engine-outage cause retained for the next attempt.
        cause_code: String,
    },
}

/// Planned terminal write that also carries the immutable product result snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoringWorkerResultAttempt {
    outcome: ScoringWorkerEngineOutcome,
    event: IntegrationEvent,
    snapshot: Option<ResultSnapshot>,
}

impl ScoringWorkerResultAttempt {
    /// Return the engine outcome that must be committed.
    #[must_use]
    pub const fn outcome(&self) -> &ScoringWorkerEngineOutcome {
        &self.outcome
    }

    /// Return the outbox envelope bound to the stable job and outcome identity.
    #[must_use]
    pub const fn event(&self) -> &IntegrationEvent {
        &self.event
    }

    /// Return the immutable result snapshot that must persist with successful completion.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&ResultSnapshot> {
        self.snapshot.as_ref()
    }
}

/// Ask the request-bound engine, then bind the snapshot and stable outbox identity.
///
/// Caller envelope fields are validated before the engine runs so an invalid
/// digest or tenant cannot spend scientific compute. A completed result must
/// match the loaded request. The product snapshot identity must reuse the
/// engine result identity so the job, snapshot, and outbox name one result.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job is invalid,
/// [`ScoringWorkerError::InvalidEnvelope`] when caller envelope fields cannot
/// form an integration event, [`ScoringWorkerError::MismatchedScoringResult`]
/// when the engine result is bound to another request,
/// [`ScoringWorkerError::InvalidResultSnapshot`] when the snapshot cannot be
/// built, or the engine's own identity error. A retryable engine outage returns
/// [`ScoringWorkerResultPlan::Retryable`] without binding an `event_ref`.
pub fn plan_scoring_worker_result_attempt(
    scoring_job_ref: &str,
    request: &ScoringRequest,
    engine: &impl ScoringWorkerResultEngine,
    snapshot_input: ResultSnapshotInput<'_>,
    envelope: ScoringWorkerEnvelope<'_>,
) -> Result<ScoringWorkerResultPlan, ScoringWorkerError> {
    let scoring_job_ref = required_reference(scoring_job_ref)?;
    require_valid_worker_envelope(scoring_job_ref, envelope)?;
    let outcome = engine.score_claimed_request(scoring_job_ref, request)?;
    match outcome {
        ScoringWorkerResultOutcome::Completed { result } => {
            if !result.matches_request(request) {
                return Err(ScoringWorkerError::MismatchedScoringResult);
            }
            if snapshot_input.result_snapshot_ref != result.scoring_result_ref() {
                return Err(ScoringWorkerError::MismatchedScoringResult);
            }
            let snapshot = ResultSnapshot::new(request, result.as_ref(), snapshot_input)
                .map_err(map_snapshot_error)?;
            let event_ref = scoring_terminal_event_ref(
                scoring_job_ref,
                ScoringTerminalIdentity::Result(result.scoring_result_ref()),
            )?;
            let event = bind_worker_event(scoring_job_ref, &event_ref, envelope)?;
            Ok(ScoringWorkerResultPlan::Terminal(Box::new(
                ScoringWorkerResultAttempt {
                    outcome: ScoringWorkerEngineOutcome::Completed {
                        result_ref: result.scoring_result_ref().to_owned(),
                    },
                    event,
                    snapshot: Some(snapshot),
                },
            )))
        }
        ScoringWorkerResultOutcome::Failed { cause_code } => {
            let event_ref = scoring_terminal_event_ref(
                scoring_job_ref,
                ScoringTerminalIdentity::Cause(&cause_code),
            )?;
            let event = bind_worker_event(scoring_job_ref, &event_ref, envelope)?;
            Ok(ScoringWorkerResultPlan::Terminal(Box::new(
                ScoringWorkerResultAttempt {
                    outcome: ScoringWorkerEngineOutcome::Failed { cause_code },
                    event,
                    snapshot: None,
                },
            )))
        }
        ScoringWorkerResultOutcome::Retryable { cause_code } => {
            let cause_code = required_reference(&cause_code)?.to_owned();
            Ok(ScoringWorkerResultPlan::Retryable { cause_code })
        }
    }
}

fn bind_worker_event(
    scoring_job_ref: &str,
    event_ref: &str,
    envelope: ScoringWorkerEnvelope<'_>,
) -> Result<IntegrationEvent, ScoringWorkerError> {
    IntegrationEvent::new(
        event_ref,
        envelope.event_type,
        envelope.schema_version,
        envelope.source,
        envelope.tenant_ref,
        scoring_job_ref,
        envelope.occurred_at_unix_ms,
        envelope.correlation_ref,
        envelope.causation_ref,
        envelope.payload_digest,
    )
    .map_err(|_| ScoringWorkerError::InvalidEnvelope)
}

fn map_snapshot_error(error: ResultSnapshotError) -> ScoringWorkerError {
    match error {
        ResultSnapshotError::ScoringRequestMismatch => ScoringWorkerError::MismatchedScoringResult,
        ResultSnapshotError::EmptyReference
        | ResultSnapshotError::MissingConsentSnapshot
        | ResultSnapshotError::DuplicateConsentSnapshot
        | ResultSnapshotError::InvalidCreationTime
        | ResultSnapshotError::SelfSupersession => ScoringWorkerError::InvalidResultSnapshot,
    }
}
