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
    let mut ledger = ResponseLedger::new("session_ref");
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "event_ref",
                client_event_ref: "client_ref",
                item_version_ref: "item_version_ref",
                payload_digest: "sha256:response",
            },
        )
        .unwrap();
    ledger.freeze(SessionState::Completed).unwrap()
}

fn request(assessment_spec_ref: &str) -> ScoringRequest {
    ScoringRequest::from_snapshot(
        &completed_snapshot(),
        ScoringRequestInput {
            scoring_request_ref: "shared_request_ref",
            response_snapshot_ref: "response_snapshot_ref",
            assessment_spec_ref,
            instrument_version_ref: "instrument_version_ref",
            scoring_version_ref: "scoring_version_ref",
            calibration_reference: "calibration_reference",
            norm_version_ref: Some("norm_version_ref"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

#[test]
fn result_snapshot_rejects_same_request_reference_with_different_provenance() {
    let expected_request = request("assessment_spec_a");
    let conflicting_request = request("assessment_spec_b");
    let result = ScoringResult::new(
        "scoring_result_ref",
        &conflicting_request,
        "sha256:engine",
        vec![ScoreObservation::scored("construct_ref", 1.0, Some(0.2)).unwrap()],
    )
    .unwrap();

    let error = ResultSnapshot::new(
        &expected_request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["service_consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
    )
    .unwrap_err();

    assert_eq!(error, ResultSnapshotError::ScoringRequestMismatch);
}
