//! Contract tests for typed scientific failures at the scoring-engine boundary.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput, ScoringResult};
use psychometrics_commons_runtime::scoring_engine::{
    execute_scoring_request, ScientificScoringFailure, ScoringEngine, ScoringEngineExecutionError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
struct UpstreamScientificFailure;

impl Display for UpstreamScientificFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("upstream scientific failure details")
    }
}

impl Error for UpstreamScientificFailure {}

struct ScientificFailureEngine {
    failure: ScientificScoringFailure,
}

impl ScoringEngine for ScientificFailureEngine {
    type Error = UpstreamScientificFailure;

    fn score(&self, _request: &ScoringRequest) -> Result<ScoringResult, Self::Error> {
        Err(UpstreamScientificFailure)
    }

    fn classify_scientific_failure(
        &self,
        _error: &Self::Error,
    ) -> Option<ScientificScoringFailure> {
        Some(self.failure)
    }
}

fn scoring_request() -> ScoringRequest {
    let mut ledger = ResponseLedger::new("session_scientific_failure").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_scientific_failure",
                client_event_ref: "client_event_scientific_failure",
                item_version_ref: "item_version_scientific_failure",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(
            SessionState::Completed,
            "response_snapshot_scientific_failure",
        )
        .unwrap();

    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_scientific_failure",
            response_snapshot_ref: "response_snapshot_scientific_failure",
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

#[test]
fn adapter_preserves_typed_scientific_failure_without_inventing_a_result() {
    let request = scoring_request();
    let cases = [
        (ScientificScoringFailure::InvalidModel, "invalid_model"),
        (
            ScientificScoringFailure::UnknownModelRelation,
            "unknown_model_relation",
        ),
        (
            ScientificScoringFailure::NonIdentification,
            "non_identification",
        ),
        (
            ScientificScoringFailure::InsufficientLinkingAnchors,
            "insufficient_linking_anchors",
        ),
        (
            ScientificScoringFailure::NonFiniteEstimate,
            "non_finite_estimate",
        ),
        (
            ScientificScoringFailure::ScoreabilityFailure,
            "scoreability_failure",
        ),
    ];

    for (failure, expected_code) in cases {
        assert_eq!(failure.code(), expected_code);
        let engine = ScientificFailureEngine { failure };
        let error = execute_scoring_request(&engine, &request).unwrap_err();

        assert_eq!(error.scientific_failure(), Some(failure));
        assert!(matches!(
            error,
            ScoringEngineExecutionError::Scientific {
                failure: actual,
                ..
            } if actual == failure
        ));
        assert_eq!(
            error.to_string(),
            "scoring engine rejected the request for a scientific reason"
        );
        assert_eq!(
            error.source().map(ToString::to_string),
            Some("upstream scientific failure details".to_owned())
        );
    }
}
