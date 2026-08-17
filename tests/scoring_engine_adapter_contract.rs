//! Contract tests for the product-owned scoring-engine adapter boundary.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::scoring_engine::{
    execute_scoring_request, ScoringEngine, ScoringEngineExecutionError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::fmt::{Display, Formatter};

const ENGINE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn completed_snapshot() -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut ledger = ResponseLedger::new("session_scoring_adapter").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_scoring_adapter",
                client_event_ref: "client_event_scoring_adapter",
                item_version_ref: "item_version_scoring_adapter",
                payload_digest:
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            },
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "response_snapshot_scoring_adapter")
        .unwrap()
}

fn request_with_ref(request_ref: &'static str) -> ScoringRequest {
    ScoringRequest::from_snapshot(
        &completed_snapshot(),
        ScoringRequestInput {
            scoring_request_ref: request_ref,
            response_snapshot_ref: "response_snapshot_scoring_adapter",
            assessment_spec_ref: "assessment_spec_big_five",
            instrument_version_ref: "instrument_version_big_five_en_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn result_for(request: &ScoringRequest, result_ref: &str) -> ScoringResult {
    ScoringResult::new(
        result_ref,
        request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap()
}

#[derive(Debug)]
struct EngineUnavailable;

impl Display for EngineUnavailable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("scoring engine unavailable")
    }
}

impl Error for EngineUnavailable {}

struct SuccessfulEngine;

impl ScoringEngine for SuccessfulEngine {
    type Error = EngineUnavailable;

    fn score(&self, request: &ScoringRequest) -> Result<ScoringResult, Self::Error> {
        Ok(result_for(request, "scoring_result_success"))
    }
}

struct MismatchedEngine;

impl ScoringEngine for MismatchedEngine {
    type Error = EngineUnavailable;

    fn score(&self, _request: &ScoringRequest) -> Result<ScoringResult, Self::Error> {
        let other_request = request_with_ref("scoring_request_other");
        Ok(result_for(&other_request, "scoring_result_other"))
    }
}

struct UnavailableEngine;

impl ScoringEngine for UnavailableEngine {
    type Error = EngineUnavailable;

    fn score(&self, _request: &ScoringRequest) -> Result<ScoringResult, Self::Error> {
        Err(EngineUnavailable)
    }
}

#[test]
fn adapter_returns_only_a_result_bound_to_the_exact_request() {
    let request = request_with_ref("scoring_request_primary");
    let result = execute_scoring_request(&SuccessfulEngine, &request).unwrap();

    assert_eq!(result.scoring_request_ref(), "scoring_request_primary");
    assert_eq!(result.response_snapshot_ref(), "response_snapshot_scoring_adapter");
    assert_eq!(result.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(result.observations()[0].score(), Some(0.42));
}

#[test]
fn adapter_rejects_an_engine_result_bound_to_another_request() {
    let request = request_with_ref("scoring_request_primary");
    let error = execute_scoring_request(&MismatchedEngine, &request).unwrap_err();

    assert!(matches!(
        error,
        ScoringEngineExecutionError::RequestMismatch
    ));
    assert_eq!(
        error.to_string(),
        "scoring engine result does not belong to the dispatched request"
    );
    assert!(error.source().is_none());
}

#[test]
fn adapter_preserves_engine_failure_as_the_error_source() {
    let request = request_with_ref("scoring_request_primary");
    let error = execute_scoring_request(&UnavailableEngine, &request).unwrap_err();

    assert!(matches!(error, ScoringEngineExecutionError::Engine(_)));
    assert_eq!(error.to_string(), "scoring engine execution failed");
    assert_eq!(
        error.source().map(ToString::to_string),
        Some("scoring engine unavailable".to_owned())
    );
}
