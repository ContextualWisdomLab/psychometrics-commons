//! Regression for conflicting provenance hidden behind one scoring request reference.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

fn completed_snapshot() -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "event_ref",
                client_event_ref: "client_ref",
                item_version_ref: "item_version_ref",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap()
}

fn request(
    assessment_spec_ref: &str,
    norm_version_ref: Option<&str>,
    scoring_version_ref: &str,
) -> ScoringRequest {
    ScoringRequest::from_snapshot(
        &completed_snapshot(),
        ScoringRequestInput {
            scoring_request_ref: "shared_request_ref",
            response_snapshot_ref: "response_snapshot_ref",
            assessment_spec_ref,
            instrument_version_ref: "instrument_version_ref",
            scoring_version_ref,
            calibration_reference: "calibration_reference",
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn result_for(request: &ScoringRequest) -> ScoringResult {
    ScoringResult::new(
        "scoring_result_ref",
        request,
        "sha256:engine",
        vec![ScoreObservation::scored("construct_ref", 1.0, Some(0.2)).unwrap()],
    )
    .unwrap()
}

fn result_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_snapshot_ref",
        participant_ref: "participant_ref",
        narrative_version_ref: "narrative_version_ref",
        consent_snapshot_refs: &["service_consent_ref"],
        created_at_unix_ms: 1,
        supersedes_ref: None,
    }
}

#[test]
fn result_snapshot_rejects_same_request_reference_with_different_assessment_spec() {
    let expected_request = request(
        "assessment_spec_a",
        Some("norm_version_ref"),
        "scoring_version_ref",
    );
    let conflicting_request = request(
        "assessment_spec_b",
        Some("norm_version_ref"),
        "scoring_version_ref",
    );
    let result = result_for(&conflicting_request);

    let error = ResultSnapshot::new(&expected_request, &result, result_input()).unwrap_err();

    assert_eq!(error, ResultSnapshotError::ScoringRequestMismatch);
}

#[test]
fn result_snapshot_rejects_same_request_reference_with_different_optional_norm() {
    let expected_request = request(
        "assessment_spec_ref",
        Some("norm_version_ref"),
        "scoring_version_ref",
    );
    let conflicting_request = request("assessment_spec_ref", None, "scoring_version_ref");
    let result = result_for(&conflicting_request);

    let error = ResultSnapshot::new(&expected_request, &result, result_input()).unwrap_err();

    assert_eq!(error, ResultSnapshotError::ScoringRequestMismatch);
}

#[test]
fn result_snapshot_rejects_same_request_reference_with_different_scoring_version() {
    let expected_request = request(
        "assessment_spec_ref",
        Some("norm_version_ref"),
        "scoring_version_a",
    );
    let conflicting_request = request(
        "assessment_spec_ref",
        Some("norm_version_ref"),
        "scoring_version_b",
    );
    let result = result_for(&conflicting_request);

    let error = ResultSnapshot::new(&expected_request, &result, result_input()).unwrap_err();

    assert_eq!(error, ResultSnapshotError::ScoringRequestMismatch);
}
