//! Released measurement-coordinate provenance distinguishes construct authority.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::measurement_coordinate::{
    MEASUREMENT_COORDINATE_CONTRACT_VERSION, MeasurementCoordinateProvenance,
};
use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn result_snapshot() -> ResultSnapshot {
    let response_snapshot = frozen_snapshot(
        "session_measurement_coordinate",
        "response_snapshot_measurement_coordinate",
        &[ResponseWrite {
            server_event_ref: "event_measurement_coordinate",
            client_event_ref: "client_measurement_coordinate",
            item_version_ref: "item_measurement_coordinate",
            payload_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_measurement_coordinate",
            response_snapshot_ref: "response_snapshot_measurement_coordinate",
            assessment_spec_ref: "assessment_spec_measurement_coordinate_v1",
            instrument_version_ref: "instrument_measurement_coordinate_v1",
            scoring_version_ref: "scoring_measurement_coordinate_v1",
            calibration_reference: "calibration_measurement_coordinate_v1",
            norm_version_ref: Some("norm_measurement_coordinate_v1"),
            requested_output_schema_version: 1,
        },
    )
    .expect("measurement-coordinate scoring request");
    let result = ScoringResult::new(
        "scoring_result_measurement_coordinate",
        &request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored("construct_predictor", 0.25, Some(0.05))
                .expect("predictor observation"),
            ScoreObservation::scored("construct_outcome", 0.25, Some(0.05))
                .expect("outcome observation"),
            ScoreObservation::without_score("construct_abstained", ObservationDisposition::Abstained)
                .expect("abstained observation"),
        ],
    )
    .expect("measurement-coordinate scoring result");
    ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_measurement_coordinate",
            participant_ref: "participant_measurement_coordinate",
            narrative_version_ref: "narrative_measurement_coordinate_v1",
            consent_snapshot_refs: &["consent_measurement_coordinate"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
    )
    .expect("measurement-coordinate result snapshot")
}

#[test]
fn construct_identity_changes_coordinate_authority_when_numeric_values_match() {
    let snapshot = result_snapshot();
    let predictor = MeasurementCoordinateProvenance::from_result_snapshot(
        &snapshot,
        "construct_predictor",
    )
    .expect("predictor provenance");
    let outcome = MeasurementCoordinateProvenance::from_result_snapshot(
        &snapshot,
        "construct_outcome",
    )
    .expect("outcome provenance");

    assert_eq!(predictor.contract_version(), MEASUREMENT_COORDINATE_CONTRACT_VERSION);
    assert_eq!(predictor.assessment_spec_ref(), "assessment_spec_measurement_coordinate_v1");
    assert_eq!(predictor.instrument_version_ref(), "instrument_measurement_coordinate_v1");
    assert_eq!(predictor.scoring_version_ref(), "scoring_measurement_coordinate_v1");
    assert_eq!(predictor.calibration_reference(), "calibration_measurement_coordinate_v1");
    assert_eq!(predictor.norm_version_ref(), Some("norm_measurement_coordinate_v1"));
    assert_eq!(predictor.requested_output_schema_version(), 1);
    assert_eq!(predictor.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(predictor.construct_ref(), "construct_predictor");
    assert_ne!(predictor.canonical_bytes(), outcome.canonical_bytes());
}

#[test]
fn projection_is_participant_free_and_rejects_unscored_or_unknown_constructs() {
    let snapshot = result_snapshot();
    let predictor = MeasurementCoordinateProvenance::from_result_snapshot(
        &snapshot,
        "construct_predictor",
    )
    .expect("predictor provenance");
    let bytes = predictor.canonical_bytes();
    let text = String::from_utf8(bytes).expect("canonical provenance is UTF-8-safe");

    assert!(!text.contains("participant_measurement_coordinate"));
    assert!(!text.contains("response_snapshot_measurement_coordinate"));
    assert!(!text.contains("narrative_measurement_coordinate_v1"));
    assert!(MeasurementCoordinateProvenance::from_result_snapshot(
        &snapshot,
        "construct_abstained",
    )
    .is_err());
    assert!(MeasurementCoordinateProvenance::from_result_snapshot(
        &snapshot,
        "construct_missing",
    )
    .is_err());
}
