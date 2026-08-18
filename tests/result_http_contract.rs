//! Contract tests for authorized immutable result retrieval over HTTP.

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_http::handle_result_http_request;
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn result_snapshot(participant_ref: &str) -> ResultSnapshot {
    let mut ledger = ResponseLedger::new("session_result_http").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_result_http",
                client_event_ref: "client_event_result_http",
                item_version_ref: "item_version_result_http",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let response_snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_result_http")
        .unwrap();
    let scoring_request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_http",
            response_snapshot_ref: "response_snapshot_result_http",
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
        "scoring_result_result_http",
        &scoring_request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored("big_five_extraversion", 0.42, Some(0.18)).unwrap(),
            ScoreObservation::without_score(
                "big_five_neuroticism",
                ObservationDisposition::Excluded,
            )
            .unwrap(),
        ],
    )
    .unwrap();

    ResultSnapshot::new(
        &scoring_request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_result_http",
            participant_ref,
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["service_consent_result_http"],
            created_at_unix_ms: 1_786_240_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn participant(tenant_ref: &str, participant_ref: &str) -> ParticipantRecord {
    ParticipantRecord::new_anonymous(participant_ref, tenant_ref, 1).unwrap()
}

fn actor(tenant_ref: &str, participant_ref: Option<&str>) -> AuthorizationContext {
    AuthorizationContext::new(
        tenant_ref,
        "subject_result_http",
        participant_ref,
        &[ProductRole::Participant],
    )
    .unwrap()
}

#[test]
fn authorized_owner_reads_the_exact_immutable_score_and_provenance() {
    let participant = participant("tenant_alpha", "participant_alpha");
    let result = result_snapshot("participant_alpha");
    let actor = actor("tenant_alpha", Some("participant_alpha"));

    let response = handle_result_http_request(
        "GET /v1/results/result_snapshot_result_http HTTP/1.1\r\nHost: example.test\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/json");
    assert!(response.body().contains("\"result_ref\":\"result_snapshot_result_http\""));
    assert!(response.body().contains("\"participant_ref\":\"participant_alpha\""));
    assert!(response.body().contains("\"instrument_version_ref\":\"instrument_version_big_five_ko_v1\""));
    assert!(response.body().contains("\"score\":0.42"));
    assert!(response.body().contains("\"standard_error\":0.18"));
    assert!(response.body().contains("\"disposition\":\"excluded\",\"score\":null,\"standard_error\":null"));
}

#[test]
fn cross_tenant_caller_is_denied_without_result_identity_or_score_leakage() {
    let participant = participant("tenant_alpha", "participant_alpha");
    let result = result_snapshot("participant_alpha");
    let actor = actor("tenant_beta", Some("participant_alpha"));

    let response = handle_result_http_request(
        "GET /v1/results/result_snapshot_other HTTP/1.1\r\nHost: example.test\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(response.status(), 403);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(!response.body().contains("result_snapshot_result_http"));
    assert!(!response.body().contains("0.42"));
}

#[test]
fn authorized_owner_cannot_rebind_the_route_to_another_result() {
    let participant = participant("tenant_alpha", "participant_alpha");
    let result = result_snapshot("participant_alpha");
    let actor = actor("tenant_alpha", Some("participant_alpha"));

    let response = handle_result_http_request(
        "GET /v1/results/result_snapshot_other HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );

    assert_eq!(response.status(), 404);
    assert!(!response.body().contains("participant_alpha"));
}

#[test]
fn invalid_route_alias_and_unsupported_method_fail_closed() {
    let participant = participant("tenant_alpha", "participant_alpha");
    let result = result_snapshot("participant_alpha");
    let actor = actor("tenant_alpha", Some("participant_alpha"));

    for target in [
        "/v1/results/12345",
        "/v1/results/%72esult_snapshot_result_http",
        "/v1/results/result_snapshot_result_http/extra",
    ] {
        let response = handle_result_http_request(
            &format!("GET {target} HTTP/1.1\r\n\r\n"),
            &actor,
            &participant,
            &result,
        );
        assert!(matches!(response.status(), 400 | 404));
        assert!(!response.body().contains("0.42"));
    }

    let response = handle_result_http_request(
        "POST /v1/results/result_snapshot_result_http HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );
    assert_eq!(response.status(), 405);
}

#[test]
fn malformed_request_and_wrong_stored_owner_fail_closed() {
    let participant = participant("tenant_alpha", "participant_beta");
    let result = result_snapshot("participant_alpha");
    let actor = actor("tenant_alpha", Some("participant_beta"));

    let malformed = handle_result_http_request("GET\r\n\r\n", &actor, &participant, &result);
    assert_eq!(malformed.status(), 400);

    let denied = handle_result_http_request(
        "GET /v1/results/result_snapshot_result_http HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &result,
    );
    assert_eq!(denied.status(), 403);
    assert!(!denied.body().contains("participant_alpha"));
}
