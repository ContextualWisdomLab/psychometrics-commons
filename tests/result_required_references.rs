//! Coverage and fail-closed regressions for required immutable result references.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn request_and_result() -> (ScoringRequest, ScoringResult) {
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
    let snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_ref",
            response_snapshot_ref: "response_snapshot_ref",
            assessment_spec_ref: "assessment_spec_ref",
            instrument_version_ref: "instrument_version_ref",
            scoring_version_ref: "scoring_version_ref",
            calibration_reference: "calibration_reference",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_ref",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_ref", 0.0, None).unwrap()],
    )
    .unwrap();
    (request, result)
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
fn result_snapshot_rejects_blank_snapshot_identity() {
    let (request, result) = request_and_result();
    let mut input = result_input();
    input.result_snapshot_ref = "   ";

    assert_eq!(
        ResultSnapshot::new(&request, &result, input).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );
}

#[test]
fn result_snapshot_rejects_blank_narrative_version() {
    let (request, result) = request_and_result();
    let mut input = result_input();
    input.narrative_version_ref = "\t";

    assert_eq!(
        ResultSnapshot::new(&request, &result, input).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );
}

#[test]
fn result_snapshot_normalizes_edge_whitespace_before_identity_becomes_durable() {
    let (request, result) = request_and_result();
    let mut input = result_input();
    input.result_snapshot_ref = "  result_snapshot_ref  ";
    input.participant_ref = "\u{00a0}participant_ref\u{00a0}";
    input.narrative_version_ref = "\u{2003}narrative_version_ref\u{2003}";
    input.consent_snapshot_refs = &["\u{00a0}service_consent_ref\u{00a0}"];

    let snapshot = ResultSnapshot::new(&request, &result, input).unwrap();

    assert_eq!(snapshot.result_snapshot_ref(), "result_snapshot_ref");
    assert_eq!(snapshot.participant_ref(), "participant_ref");
    assert_eq!(snapshot.narrative_version_ref(), "narrative_version_ref");
    assert_eq!(snapshot.consent_snapshot_refs(), &["service_consent_ref"]);
}
