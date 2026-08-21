//! Personal result exports preserve causal time ordering with their source result.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{
    ResultExport, ResultExportError, ResultExportInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const RESULT_CREATED_AT_UNIX_MS: u64 = 1_700_000_000_000;
const ENGINE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn snapshot() -> ResultSnapshot {
    let mut ledger = ResponseLedger::new("session_export_time_order").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "event_export_time_order",
                client_event_ref: "client_export_time_order",
                item_version_ref: "item_export_time_order",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let response_snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_export_time_order")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_export_time_order",
            response_snapshot_ref: "response_snapshot_export_time_order",
            assessment_spec_ref: "assessment_spec_export_time_order",
            instrument_version_ref: "instrument_version_export_time_order",
            scoring_version_ref: "scoring_version_export_time_order",
            calibration_reference: "calibration_export_time_order",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_export_time_order",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_export_time_order", 0.5, Some(0.1)).unwrap()],
    )
    .unwrap();

    ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_export_time_order",
            participant_ref: "participant_export_time_order",
            narrative_version_ref: "narrative_export_time_order",
            consent_snapshot_refs: &["consent_export_time_order"],
            created_at_unix_ms: RESULT_CREATED_AT_UNIX_MS,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn export_at(
    snapshot: &ResultSnapshot,
    exported_at_unix_ms: u64,
) -> Result<ResultExport, ResultExportError> {
    ResultExport::from_snapshot(
        snapshot,
        ResultExportInput {
            export_ref: "result_export_time_order",
            locale: "en-US",
            exported_at_unix_ms,
            limitations: &["This export is not a diagnosis."],
        },
    )
}

#[test]
fn export_before_result_creation_fails_closed() {
    let snapshot = snapshot();

    assert_eq!(
        export_at(&snapshot, RESULT_CREATED_AT_UNIX_MS - 1).unwrap_err(),
        ResultExportError::InvalidTimestamp
    );
}

#[test]
fn export_at_result_creation_time_is_allowed() {
    let snapshot = snapshot();

    assert!(export_at(&snapshot, RESULT_CREATED_AT_UNIX_MS).is_ok());
}
