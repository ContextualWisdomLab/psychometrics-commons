//! Released measurement-coordinate provenance distinguishes construct authority.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::measurement_coordinate::{
    MEASUREMENT_COORDINATE_CONTRACT_VERSION, MeasurementCoordinateProvenance,
    MeasurementCoordinateProvenanceError,
};
use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const OTHER_ENGINE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn result_snapshot() -> ResultSnapshot {
    result_snapshot_with(
        "assessment_spec_measurement_coordinate_v1",
        "instrument_measurement_coordinate_v1",
        "scoring_measurement_coordinate_v1",
        "calibration_measurement_coordinate_v1",
        Some("norm_measurement_coordinate_v1"),
        ENGINE_DIGEST,
    )
}

fn result_snapshot_with(
    assessment_spec_ref: &str,
    instrument_version_ref: &str,
    scoring_version_ref: &str,
    calibration_reference: &str,
    norm_version_ref: Option<&str>,
    engine_digest: &str,
) -> ResultSnapshot {
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
            assessment_spec_ref,
            instrument_version_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .expect("measurement-coordinate scoring request");
    let result = ScoringResult::new(
        "scoring_result_measurement_coordinate",
        &request,
        engine_digest,
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

fn coordinate(snapshot: &ResultSnapshot, construct_ref: &str) -> MeasurementCoordinateProvenance {
    MeasurementCoordinateProvenance::from_result_snapshot(snapshot, construct_ref)
        .expect("measurement-coordinate provenance")
}

#[test]
fn construct_identity_changes_coordinate_authority_when_numeric_values_match() {
    let snapshot = result_snapshot();
    let predictor = coordinate(&snapshot, "construct_predictor");
    let outcome = coordinate(&snapshot, "construct_outcome");

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
fn every_supported_measurement_provenance_change_changes_canonical_identity() {
    let baseline_snapshot = result_snapshot();
    let baseline = coordinate(&baseline_snapshot, "construct_predictor").canonical_bytes();
    let variants = [
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v2",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v1",
            Some("norm_measurement_coordinate_v1"),
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v2",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v1",
            Some("norm_measurement_coordinate_v1"),
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v2",
            "calibration_measurement_coordinate_v1",
            Some("norm_measurement_coordinate_v1"),
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v2",
            Some("norm_measurement_coordinate_v1"),
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v1",
            Some("norm_measurement_coordinate_v2"),
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v1",
            None,
            ENGINE_DIGEST,
        ),
        result_snapshot_with(
            "assessment_spec_measurement_coordinate_v1",
            "instrument_measurement_coordinate_v1",
            "scoring_measurement_coordinate_v1",
            "calibration_measurement_coordinate_v1",
            Some("norm_measurement_coordinate_v1"),
            OTHER_ENGINE_DIGEST,
        ),
    ];

    for variant in &variants {
        assert_ne!(
            baseline,
            coordinate(variant, "construct_predictor").canonical_bytes()
        );
    }
}

#[test]
fn projection_is_participant_free_and_rejects_invalid_unscored_or_unknown_constructs() {
    let snapshot = result_snapshot();
    let predictor = coordinate(&snapshot, "construct_predictor");
    let bytes = predictor.canonical_bytes();
    let text = String::from_utf8(bytes).expect("canonical provenance is UTF-8-safe");

    assert!(!text.contains("participant_measurement_coordinate"));
    assert!(!text.contains("response_snapshot_measurement_coordinate"));
    assert!(!text.contains("narrative_measurement_coordinate_v1"));
    assert_eq!(
        MeasurementCoordinateProvenance::from_result_snapshot(
            &snapshot,
            " construct_predictor ",
        )
        .unwrap_err(),
        MeasurementCoordinateProvenanceError::InvalidConstructReference
    );
    assert_eq!(
        MeasurementCoordinateProvenance::from_result_snapshot(&snapshot, "construct_abstained")
            .unwrap_err(),
        MeasurementCoordinateProvenanceError::UnscoredConstruct
    );
    assert_eq!(
        MeasurementCoordinateProvenance::from_result_snapshot(&snapshot, "construct_missing")
            .unwrap_err(),
        MeasurementCoordinateProvenanceError::UnknownConstruct
    );
}

#[test]
fn error_messages_preserve_distinct_operator_causes() {
    assert!(MeasurementCoordinateProvenanceError::InvalidConstructReference
        .to_string()
        .contains("exact safe opaque spelling"));
    assert!(MeasurementCoordinateProvenanceError::UnknownConstruct
        .to_string()
        .contains("absent"));
    assert!(MeasurementCoordinateProvenanceError::UnscoredConstruct
        .to_string()
        .contains("scored construct"));
}
