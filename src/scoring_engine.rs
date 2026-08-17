//! Product-owned adapter boundary for invoking an external scoring engine.
//!
//! Psychometrics Commons owns orchestration and provenance validation, not the
//! psychometric numerical implementation. Implementations of [`ScoringEngine`]
//! therefore call a versioned `fast-mlsirm`-compatible boundary and return the
//! already-validated product scoring contract. This module prevents an adapter
//! from publishing a result for a different immutable scoring request.

use crate::scoring::{ScoringRequest, ScoringResult};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Versioned scoring-engine boundary consumed by the hosted product runtime.
///
/// Implementations may be in-process package adapters or remote service
/// clients, but they must not recompute psychometric quantities in this
/// repository. Provider-specific errors remain available as an error source
/// for operator diagnostics while the public adapter message stays stable.
pub trait ScoringEngine {
    /// Typed provider, transport, or scientific execution error.
    type Error: Error + Send + Sync + 'static;

    /// Execute one immutable scoring request through the external engine.
    ///
    /// The returned [`ScoringResult`] must be bound to the exact supplied
    /// request. [`execute_scoring_request`] enforces that invariant before the
    /// product may persist or publish the result.
    ///
    /// # Errors
    ///
    /// Returns the implementation-defined engine error when execution cannot
    /// produce a contract-valid result.
    fn score(&self, request: &ScoringRequest) -> Result<ScoringResult, Self::Error>;
}

/// Fail-closed error from the product scoring-engine adapter boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringEngineExecutionError<E> {
    /// The external scoring implementation did not produce a result.
    Engine(E),
    /// The engine returned a result belonging to another immutable request.
    RequestMismatch,
}

impl<E> Display for ScoringEngineExecutionError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Engine(_) => "scoring engine execution failed",
            Self::RequestMismatch => {
                "scoring engine result does not belong to the dispatched request"
            }
        })
    }
}

impl<E> Error for ScoringEngineExecutionError<E>
where
    E: Error + Send + Sync + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Engine(error) => Some(error),
            Self::RequestMismatch => None,
        }
    }
}

/// Execute one scoring request and enforce exact request/result provenance.
///
/// This function intentionally performs no score calculation and no fallback
/// scoring. A provider outage remains an engine failure, and a result produced
/// for another request is rejected rather than rebound to the caller's request.
///
/// # Errors
///
/// Returns [`ScoringEngineExecutionError::Engine`] when the engine fails, or
/// [`ScoringEngineExecutionError::RequestMismatch`] when the returned result is
/// not bound to the complete dispatched request.
pub fn execute_scoring_request<E>(
    engine: &E,
    request: &ScoringRequest,
) -> Result<ScoringResult, ScoringEngineExecutionError<E::Error>>
where
    E: ScoringEngine,
{
    let result = engine
        .score(request)
        .map_err(ScoringEngineExecutionError::Engine)?;
    if !result.matches_request(request) {
        return Err(ScoringEngineExecutionError::RequestMismatch);
    }
    Ok(result)
}
