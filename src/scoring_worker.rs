//! Scoring-worker terminal identity that keeps outbox evidence replay-safe.
//!
//! This module does not calculate psychometric quantities or call `fast-mlsirm`.
//! It binds one durable outbox event identity to the exact scoring job and the
//! accepted result or permanent cause so a crashed worker can reconcile without
//! minting a second event after an already-accepted terminal write.

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

fn required_reference(reference: &str) -> Result<&str, ScoringWorkerError> {
    normalized_reference(reference).ok_or(ScoringWorkerError::InvalidReference)
}
