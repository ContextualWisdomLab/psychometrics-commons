//! Contract tests for the product-owned scoring-engine adapter boundary.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringContractError, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::scoring_engine::{
    execute_scoring_request, ScoringEngine, ScoringEngineExecutionError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;
use std::fmt::{Display, Formatter};

const ENGINE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PRIMARY_SNAPSHOT_REF: &str = "response_snapshot_scoring_adapter";
const PRIMARY_SCORING_VERSION: &str = "scoring_version_big_five_v1";
const PRIMARY_ASSESSMENT_SPEC_REF: &str = "assessment_spec_big_five";
const PRIMARY_INSTRUMENT_VERSION_REF: &str = "instrument_version_big_five_en_v1";
const PRIMARY_CALIBRATION_REFERENCE: &str = "calibration_big_five_v1";

fn completed_snapshot_with_ref(
    snapshot_ref: &str,
) -> psychometrics_commons_runtime::response::ResponseSnapshot {
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
        .freeze_as(SessionState::Completed, snapshot_ref)
        .unwrap()
}

fn request_with_provenance(
    request_ref: &str,
    snapshot_ref: &str,
    assessment_spec_ref: &str,
    instrument_version_ref: &str,
    scoring_version_ref: &str,
    calibration_reference: &str,
    norm_version_ref: Option<&str>,
) -> ScoringRequest {
    let snapshot = completed_snapshot_with_ref(snapshot_ref);
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: request_ref,
            response_snapshot_ref: snapshot_ref,
            assessment_spec_ref,
            instrument_version_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn request_with_identity(
    request_ref: &str,
    snapshot_ref: &str,
    scoring_version_ref: &str,
) -> ScoringRequest {
    request_with_provenance(
        request_ref,
        snapshot_ref,
        PRIMARY_ASSESSMENT_SPEC_REF,
        PRIMARY_INSTRUMENT_VERSION_REF,
        scoring_version_ref,
        PRIMARY_CALIBRATION_REFERENCE,
        None,
    )
}

fn request_with_ref(request_ref: &str) -> ScoringRequest {
    request_with_identity(request_ref, PRIMARY_SNAPSHOT_REF, PRIMARY_SCORING_VERSION)
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

enum EngineBehavior {
    Success,
    ReturnFor(ScoringRequest),
    Unavailable,
}

struct TestEngine {
    behavior: EngineBehavior,
}

impl TestEngine {
    const fn success() -> Self {
        Self {
            behavior: EngineBehavior::Success,
        }
    }

    fn return_for(request: ScoringRequest) -> Self {
        Self {
            behavior: EngineBehavior::ReturnFor(request),
        }
    }

    const fn unavailable() -> Self {
        Self {
            behavior: EngineBehavior::Unavailable,
        }
    }
}

impl ScoringEngine for TestEngine {
    type Error = EngineUnavailable;

    fn score(&self, request: &ScoringRequest) -> Result<ScoringResult, Self::Error> {
        match &self.behavior {
            EngineBehavior::Success => Ok(result_for(request, "scoring_result_success")),
            EngineBehavior::ReturnFor(other_request) => {
                Ok(result_for(other_request, "scoring_result_other"))
            }
            EngineBehavior::Unavailable => Err(EngineUnavailable),
        }
    }
}

#[test]
fn adapter_returns_only_a_result_bound_to_the_exact_request() {
    let request = request_with_ref("scoring_request_primary");
    let engine = TestEngine::success();
    let result = execute_scoring_request(&engine, &request).unwrap();

    assert_eq!(result.scoring_request_ref(), "scoring_request_primary");
    assert_eq!(result.response_snapshot_ref(), PRIMARY_SNAPSHOT_REF);
    assert_eq!(result.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(result.observations()[0].score(), Some(0.42));
}

#[test]
fn adapter_rejects_any_result_not_bound_to_the_complete_request() {
    let request = request_with_ref("scoring_request_primary");
    let mismatches = [
        request_with_ref("scoring_request_other"),
        request_with_identity(
            "scoring_request_primary",
            PRIMARY_SNAPSHOT_REF,
            "scoring_version_big_five_v2",
        ),
        request_with_identity(
            "scoring_request_primary",
            "response_snapshot_scoring_adapter_other",
            PRIMARY_SCORING_VERSION,
        ),
        request_with_provenance(
            "scoring_request_primary",
            PRIMARY_SNAPSHOT_REF,
            "assessment_spec_big_five_other",
            PRIMARY_INSTRUMENT_VERSION_REF,
            PRIMARY_SCORING_VERSION,
            PRIMARY_CALIBRATION_REFERENCE,
            None,
        ),
        request_with_provenance(
            "scoring_request_primary",
            PRIMARY_SNAPSHOT_REF,
            PRIMARY_ASSESSMENT_SPEC_REF,
            "instrument_version_big_five_en_v2",
            PRIMARY_SCORING_VERSION,
            PRIMARY_CALIBRATION_REFERENCE,
            None,
        ),
        request_with_provenance(
            "scoring_request_primary",
            PRIMARY_SNAPSHOT_REF,
            PRIMARY_ASSESSMENT_SPEC_REF,
            PRIMARY_INSTRUMENT_VERSION_REF,
            PRIMARY_SCORING_VERSION,
            "calibration_big_five_v2",
            None,
        ),
        request_with_provenance(
            "scoring_request_primary",
            PRIMARY_SNAPSHOT_REF,
            PRIMARY_ASSESSMENT_SPEC_REF,
            PRIMARY_INSTRUMENT_VERSION_REF,
            PRIMARY_SCORING_VERSION,
            PRIMARY_CALIBRATION_REFERENCE,
            Some("norm_version_big_five_v1"),
        ),
    ];

    for mismatched_request in mismatches {
        let engine = TestEngine::return_for(mismatched_request);
        let error = execute_scoring_request(&engine, &request).unwrap_err();

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
}

#[test]
fn adapter_rejects_rebinding_between_two_pinned_norm_versions() {
    let request = request_with_provenance(
        "scoring_request_normed",
        PRIMARY_SNAPSHOT_REF,
        PRIMARY_ASSESSMENT_SPEC_REF,
        PRIMARY_INSTRUMENT_VERSION_REF,
        PRIMARY_SCORING_VERSION,
        PRIMARY_CALIBRATION_REFERENCE,
        Some("norm_version_big_five_v1"),
    );
    let mismatched_request = request_with_provenance(
        "scoring_request_normed",
        PRIMARY_SNAPSHOT_REF,
        PRIMARY_ASSESSMENT_SPEC_REF,
        PRIMARY_INSTRUMENT_VERSION_REF,
        PRIMARY_SCORING_VERSION,
        PRIMARY_CALIBRATION_REFERENCE,
        Some("norm_version_big_five_v2"),
    );

    let engine = TestEngine::return_for(mismatched_request);
    let error = execute_scoring_request(&engine, &request).unwrap_err();

    assert!(matches!(
        error,
        ScoringEngineExecutionError::RequestMismatch
    ));
    assert!(error.source().is_none());
}

#[test]
fn unsupported_output_schema_cannot_reach_the_engine_adapter() {
    let snapshot = completed_snapshot_with_ref(PRIMARY_SNAPSHOT_REF);
    let error = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_primary",
            response_snapshot_ref: PRIMARY_SNAPSHOT_REF,
            assessment_spec_ref: PRIMARY_ASSESSMENT_SPEC_REF,
            instrument_version_ref: PRIMARY_INSTRUMENT_VERSION_REF,
            scoring_version_ref: PRIMARY_SCORING_VERSION,
            calibration_reference: PRIMARY_CALIBRATION_REFERENCE,
            norm_version_ref: None,
            requested_output_schema_version: 2,
        },
    )
    .unwrap_err();

    assert_eq!(error, ScoringContractError::UnsupportedOutputSchemaVersion);
}

#[test]
fn adapter_preserves_engine_failure_as_the_error_source() {
    let request = request_with_ref("scoring_request_primary");
    let engine = TestEngine::unavailable();
    let error = execute_scoring_request(&engine, &request).unwrap_err();

    assert!(matches!(error, ScoringEngineExecutionError::Engine(_)));
    assert_eq!(error.to_string(), "scoring engine execution failed");
    assert_eq!(
        error.source().map(ToString::to_string),
        Some("scoring engine unavailable".to_owned())
    );
}
