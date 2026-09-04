//! RFC 9110 method-contract regression for immutable result reads.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_http::handle_result_http_request;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::{active_session, completed_session};

const ENGINE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn result_snapshot() -> ResultSnapshot {
    let active = active_session("session_result_method_allow");
    let mut ledger = ResponseLedger::from_session(&active).unwrap();
    ledger
        .record(
            &active,
            ResponseWrite {
                server_event_ref: "response_event_result_method_allow",
                client_event_ref: "client_event_result_method_allow",
                item_version_ref: "item_version_result_method_allow",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let completed = completed_session("session_result_method_allow");
    let response_snapshot = ledger
        .freeze_as(&completed, "response_snapshot_result_method_allow")
        .unwrap();
    let scoring_request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_method_allow",
            response_snapshot_ref: "response_snapshot_result_method_allow",
            assessment_spec_ref: "assessment_spec_big_five",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_v1",
            norm_version_ref: Some("norm_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_result_method_allow",
        &scoring_request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_extraversion", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap();

    ResultSnapshot::new(
        &scoring_request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_result_method_allow",
            participant_ref: "participant_result_method_allow",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["service_consent_result_method_allow"],
            created_at_unix_ms: 1_786_240_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn authorized_context() -> (ParticipantRecord, AuthorizationContext, ResultSnapshot) {
    let participant = ParticipantRecord::new_anonymous(
        "participant_result_method_allow",
        "tenant_result_method_allow",
        1,
    )
    .unwrap();
    let actor = AuthorizationContext::new(
        "tenant_result_method_allow",
        "subject_result_method_allow",
        Some("participant_result_method_allow"),
        &[ProductRole::Participant],
    )
    .unwrap();
    (participant, actor, result_snapshot())
}

#[test]
fn method_not_allowed_advertises_get_and_head() {
    let (participant, actor, result) = authorized_context();

    let response = handle_result_http_request(
        "POST /v1/results/result_snapshot_result_method_allow HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(response.status(), 405);
    assert_eq!(response.allow(), Some("GET, HEAD"));
}

#[test]
fn method_not_allowed_precedes_get_head_query_validation() {
    let (participant, actor, result) = authorized_context();

    let response = handle_result_http_request(
        "POST /v1/results/result_snapshot_result_method_allow?format=json HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(response.status(), 405);
    assert_eq!(response.allow(), Some("GET, HEAD"));
}

#[test]
fn head_matches_get_metadata_without_response_content() {
    let (participant, actor, result) = authorized_context();

    let get_response = handle_result_http_request(
        "GET /v1/results/result_snapshot_result_method_allow HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );
    let head_response = handle_result_http_request(
        "HEAD /v1/results/result_snapshot_result_method_allow HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(get_response.status(), 200);
    assert_eq!(head_response.status(), get_response.status());
    assert_eq!(head_response.content_type(), get_response.content_type());
    assert_eq!(head_response.allow(), get_response.allow());
    assert!(!get_response.body().is_empty());
    assert!(head_response.body().is_empty());
}
