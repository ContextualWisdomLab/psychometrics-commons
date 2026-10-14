//! Product-owned adapter boundary for invoking an external scoring engine.
//!
//! **Psychometrics Commons** is the hosted product runtime in this repository.
//! Here, **orchestration** means deciding when and how an immutable scoring
//! request is sent to a scorer; **provenance validation** means checking that
//! the returned result names the exact request and versioned evidence that was
//! dispatched. A **`fast-mlsirm`-compatible boundary** is a versioned adapter or
//! service contract whose numerical implementation lives outside this
//! repository and can consume the product scoring contract. Implementations of
//! [`ScoringEngine`] cross only that boundary: they invoke the external scorer
//! and return its already-validated product contract instead of reimplementing
//! psychometric arithmetic here. This module then prevents an adapter from
//! publishing a result for a different immutable scoring request and preserves
//! typed scientific fail-closed outcomes separately from provider availability.

use crate::scoring::{ScoringRequest, ScoringResult};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Product-level scientific reasons that make a scoring request non-scoreable.
///
/// These values classify a failure already decided by the scientific scoring
/// implementation. They do not reproduce model-selection, linking, numerical,
/// or scoreability calculations in Psychometrics Commons. A `fast-mlsirm`
/// adapter maps its typed failure into this stable product vocabulary so job
/// orchestration can quarantine deterministic scientific failures instead of
/// retrying them as provider outages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScientificScoringFailure {
    /// The requested model or model artifact is scientifically invalid.
    InvalidModel,
    /// A required relation between candidate model structures is not known.
    UnknownModelRelation,
    /// The declared model could not be identified from the supplied evidence.
    NonIdentification,
    /// Linking/equating did not have enough valid anchors to support the claim.
    InsufficientLinkingAnchors,
    /// The scientific engine produced a non-finite estimate.
    NonFiniteEstimate,
    /// The supplied response/evidence bundle is not scoreable under the policy.
    ScoreabilityFailure,
}

impl ScientificScoringFailure {
    /// Return the stable code suitable for durable job/reconciliation evidence.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidModel => "invalid_model",
            Self::UnknownModelRelation => "unknown_model_relation",
            Self::NonIdentification => "non_identification",
            Self::InsufficientLinkingAnchors => "insufficient_linking_anchors",
            Self::NonFiniteEstimate => "non_finite_estimate",
            Self::ScoreabilityFailure => "scoreability_failure",
        }
    }
}

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

    /// Classify an engine error as a deterministic scientific failure, if known.
    ///
    /// The default preserves backward compatibility for provider/transport
    /// adapters: unclassified errors remain generic engine failures. Scientific
    /// adapters should return a value only when the upstream engine has already
    /// made the corresponding fail-closed scientific determination.
    #[must_use]
    fn classify_scientific_failure(
        &self,
        _error: &Self::Error,
    ) -> Option<ScientificScoringFailure> {
        None
    }
}

/// Fail-closed error from the product scoring-engine adapter boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScoringEngineExecutionError<E> {
    /// The external scoring implementation did not produce a classified result.
    Engine(E),
    /// The scientific engine deterministically rejected the request.
    Scientific {
        /// Stable product-level scientific failure classification.
        failure: ScientificScoringFailure,
        /// Original typed engine error retained for operator diagnostics.
        source: E,
    },
    /// The engine returned a result belonging to another immutable request.
    RequestMismatch,
}

impl<E> ScoringEngineExecutionError<E> {
    /// Return the typed scientific failure, when the engine classified one.
    #[must_use]
    pub const fn scientific_failure(&self) -> Option<ScientificScoringFailure> {
        match self {
            Self::Scientific { failure, .. } => Some(*failure),
            Self::Engine(_) | Self::RequestMismatch => None,
        }
    }
}

impl<E> Display for ScoringEngineExecutionError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Engine(_) => "scoring engine execution failed",
            Self::Scientific { .. } => {
                "scoring engine rejected the request for a scientific reason"
            }
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
            Self::Engine(error) | Self::Scientific { source: error, .. } => Some(error),
            Self::RequestMismatch => None,
        }
    }
}

/// Execute one scoring request and enforce exact request/result provenance.
///
/// This function intentionally performs no score calculation and no fallback
/// scoring. A provider outage remains an engine failure. A scientific rejection
/// is returned as a typed fail-closed outcome, and a result produced for another
/// request is rejected rather than rebound to the caller's request.
///
/// # Errors
///
/// Returns [`ScoringEngineExecutionError::Scientific`] when the engine maps an
/// upstream error to a deterministic scientific failure,
/// [`ScoringEngineExecutionError::Engine`] for other engine failures, or
/// [`ScoringEngineExecutionError::RequestMismatch`] when the returned result is
/// not bound to the complete dispatched request.
pub fn execute_scoring_request<E>(
    engine: &E,
    request: &ScoringRequest,
) -> Result<ScoringResult, ScoringEngineExecutionError<E::Error>>
where
    E: ScoringEngine,
{
    let result = match engine.score(request) {
        Ok(result) => result,
        Err(error) => {
            return match engine.classify_scientific_failure(&error) {
                Some(failure) => Err(ScoringEngineExecutionError::Scientific {
                    failure,
                    source: error,
                }),
                None => Err(ScoringEngineExecutionError::Engine(error)),
            };
        }
    };
    if !result.matches_request(request) {
        return Err(ScoringEngineExecutionError::RequestMismatch);
    }
    Ok(result)
}
