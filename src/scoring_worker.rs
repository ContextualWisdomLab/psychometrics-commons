//! Scoring-worker terminal identity and scripted-engine attempt planning.
//!
//! This module does not calculate psychometric quantities or call `fast-mlsirm`.
//! It binds one durable outbox event identity to the exact scoring job and the
//! accepted result or permanent cause so a crashed worker can reconcile without
//! minting a second event after an already-accepted terminal write. A scripted
//! engine outcome is rewritten onto that identity before the terminal commit.

use crate::integration::IntegrationEvent;
use crate::reference::normalized_reference;
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
}

impl Display for ScoringWorkerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "scoring worker identities must be opaque non-numeric values",
            Self::UnstableEventRef => {
                "scoring worker must reuse the stable job and outcome event identity"
            }
        })
    }
}

impl Error for ScoringWorkerError {}

/// Return the stable outbox event identity for one terminal scoring outcome.
///
/// The same job and result, or the same job and cause, always produce the same
/// `event_ref`. A later worker attempt must reuse that identity instead of minting
/// a new event after the terminal write was already accepted.
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
        "scoring_terminal:{kind}:{scoring_job_ref}:{outcome_ref}"
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

/// Replaceable scoring-engine outcome for one fenced worker attempt.
///
/// A live `fast-mlsirm` adapter is out of scope. Tests and the first hosted worker
/// use [`ScriptedScoringEngine`] so the product can bind a stable terminal identity
/// without calculating psychometric quantities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoringEngineAttempt<'a> {
    /// The engine accepted one immutable scoring result.
    Completed {
        /// Opaque identity of the accepted scoring result.
        result_ref: &'a str,
    },
    /// The engine recorded one permanent scientific failure cause.
    PermanentFailure {
        /// Typed cause retained for quarantine and exact replay.
        cause_code: &'a str,
    },
    /// The engine failed in a retryable way and must not write a terminal outbox row.
    Retryable {
        /// Typed cause retained for the next scheduled attempt.
        cause_code: &'a str,
    },
}

/// Scripted scoring engine used by tests and the first worker attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptedScoringEngine<'a> {
    attempt: ScoringEngineAttempt<'a>,
}

impl<'a> ScriptedScoringEngine<'a> {
    /// Bind one predetermined engine outcome for a later fenced attempt.
    #[must_use]
    pub const fn new(attempt: ScoringEngineAttempt<'a>) -> Self {
        Self { attempt }
    }

    /// Return the scripted outcome without calling a psychometric kernel.
    #[must_use]
    pub const fn evaluate(&self) -> ScoringEngineAttempt<'a> {
        self.attempt
    }
}

/// Planned scoring-worker write after the stable terminal identity is bound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoringWorkerPlan<'a> {
    /// Persist a successful result with the stable job-plus-result event identity.
    Complete {
        /// Opaque identity of the accepted scoring result.
        result_ref: &'a str,
        /// Caller envelope rebuilt with the stable terminal `event_ref`.
        event: IntegrationEvent,
    },
    /// Persist a permanent failure with the stable job-plus-cause event identity.
    FailPermanently {
        /// Typed cause retained for quarantine and exact replay.
        cause_code: &'a str,
        /// Caller envelope rebuilt with the stable terminal `event_ref`.
        event: IntegrationEvent,
    },
    /// Record a retryable engine failure without a terminal outbox event.
    Retry {
        /// Typed cause retained for the next scheduled attempt.
        cause_code: &'a str,
    },
}

/// Rebuild a caller envelope so its `event_ref` is the stable job and outcome identity.
///
/// Event type, tenant, schema version, correlation, causation, and payload digest stay on
/// the supplied envelope. Only the outbox identity is replaced so a live loop cannot mint
/// a second terminal row after an accepted write.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job or outcome identity is
/// invalid.
pub fn bind_scoring_worker_terminal_event(
    scoring_job_ref: &str,
    identity: ScoringTerminalIdentity<'_>,
    envelope: &IntegrationEvent,
) -> Result<IntegrationEvent, ScoringWorkerError> {
    let event_ref = scoring_terminal_event_ref(scoring_job_ref, identity)?;
    Ok(envelope.with_event_ref(event_ref))
}

/// Plan one fenced scoring-worker attempt from a replaceable engine outcome.
///
/// Completed and permanently failed outcomes bind the stable terminal `event_ref` before
/// any write. Retryable outcomes return the typed cause and do not produce a terminal
/// outbox envelope.
///
/// # Errors
///
/// Returns [`ScoringWorkerError::InvalidReference`] when the job, result, or cause is
/// invalid.
pub fn plan_scoring_worker_attempt<'a>(
    scoring_job_ref: &str,
    attempt: ScoringEngineAttempt<'a>,
    envelope: &IntegrationEvent,
) -> Result<ScoringWorkerPlan<'a>, ScoringWorkerError> {
    match attempt {
        ScoringEngineAttempt::Completed { result_ref } => {
            let event = bind_scoring_worker_terminal_event(
                scoring_job_ref,
                ScoringTerminalIdentity::Result(result_ref),
                envelope,
            )?;
            Ok(ScoringWorkerPlan::Complete { result_ref, event })
        }
        ScoringEngineAttempt::PermanentFailure { cause_code } => {
            let event = bind_scoring_worker_terminal_event(
                scoring_job_ref,
                ScoringTerminalIdentity::Cause(cause_code),
                envelope,
            )?;
            Ok(ScoringWorkerPlan::FailPermanently { cause_code, event })
        }
        ScoringEngineAttempt::Retryable { cause_code } => {
            required_reference(scoring_job_ref)?;
            let cause_code = required_reference(cause_code)?;
            Ok(ScoringWorkerPlan::Retry { cause_code })
        }
    }
}

fn required_reference(reference: &str) -> Result<&str, ScoringWorkerError> {
    normalized_reference(reference).ok_or(ScoringWorkerError::InvalidReference)
}
