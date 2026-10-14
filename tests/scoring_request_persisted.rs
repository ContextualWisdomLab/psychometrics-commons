//! Durable scoring-request reconstruction must match the persist-time pin.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringContractError, ScoringRequest, ScoringRequestInput, ScoringResult,
};

#[path = "response_support/mod.rs"]
mod response_support;

use response_support::{active_session, completed_session};

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn completed_two_item_request() -> ScoringRequest {
    let active = active_session("session_reload_score");
    let mut ledger = ResponseLedger::from_session(&active).unwrap();
    ledger
        .record(
            &active,
            ResponseWrite {
                server_event_ref: "server_event_zzz_first",
                client_event_ref: "client_event_001",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            },
        )
        .unwrap();
    ledger
        .record(
            &active,
            ResponseWrite {
                server_event_ref: "server_event_aaa_second",
                client_event_ref: "client_event_002",
                item_version_ref: "item_version_002",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let completed = completed_session("session_reload_score");
    let snapshot = ledger
        .freeze_as(&completed, "response_snapshot_reload_score")
        .unwrap();
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_reload_score",
            response_snapshot_ref: "response_snapshot_reload_score",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn persisted_input<'a>() -> ScoringRequestInput<'a> {
    ScoringRequestInput {
        scoring_request_ref: "scoring_request_reload_score",
        response_snapshot_ref: "response_snapshot_reload_score",
        assessment_spec_ref: "assessment_spec_big_five_v1",
        instrument_version_ref: "instrument_version_big_five_ko_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        calibration_reference: "calibration_big_five_ko_v1",
        norm_version_ref: Some("norm_version_big_five_ko_v1"),
        requested_output_schema_version: 1,
    }
}

#[test]
fn persisted_identity_rebuilds_the_same_request_a_result_can_bind() {
    let frozen = completed_two_item_request();
    let rebuilt = ScoringRequest::from_persisted("session_reload_score", persisted_input())
        .expect("stored version pins must reconstruct");

    assert_eq!(rebuilt, frozen);
    let result = ScoringResult::new(
        "scoring_result_reload_score",
        &rebuilt,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
    )
    .expect("a reloaded request must still accept a typed result");
    assert_eq!(result.scoring_request_ref(), "scoring_request_reload_score");
    assert_eq!(
        result.response_snapshot_ref(),
        "response_snapshot_reload_score"
    );
}

#[test]
fn persisted_reconstruction_keeps_optional_norm_absent_when_stored_null() {
    let rebuilt = ScoringRequest::from_persisted(
        "session_reload_score",
        ScoringRequestInput {
            norm_version_ref: None,
            ..persisted_input()
        },
    )
    .unwrap();

    assert_eq!(rebuilt.norm_version_ref(), None);
    assert_eq!(rebuilt.session_ref(), "session_reload_score");
}

#[test]
fn persisted_reconstruction_rejects_blank_refs_and_unsupported_schema() {
    assert_eq!(
        ScoringRequest::from_persisted(" ", persisted_input()).unwrap_err(),
        ScoringContractError::EmptyReference
    );
    assert_eq!(
        ScoringRequest::from_persisted("42", persisted_input()).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.scoring_request_ref = " ";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.response_snapshot_ref = "";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.assessment_spec_ref = "   ";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.instrument_version_ref = "";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.scoring_version_ref = "";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.calibration_reference = "";
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.norm_version_ref = Some("");
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = persisted_input();
    input.requested_output_schema_version = 2;
    assert_eq!(
        ScoringRequest::from_persisted("session_reload_score", input).unwrap_err(),
        ScoringContractError::UnsupportedOutputSchemaVersion
    );
}
