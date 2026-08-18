//! Public result-export HTTP must preserve immutable score evidence and fail closed.

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{ResultExport, ResultExportInput};
use psychometrics_commons_runtime::result_export_http::handle_result_export_http_request;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const LIMITATION: &str = "This result is not a diagnosis or employment-fitness decision.";

fn result_snapshot(result_ref: &str, participant_ref: &str, suffix: &str) -> ResultSnapshot {
    let session_ref = format!("session_{suffix}");
    let event_ref = format!("event_{suffix}");
    let client_event_ref = format!("client_{suffix}");
    let item_version_ref = format!("item_{suffix}");
    let response_snapshot_ref = format!("response_snapshot_{suffix}");
    let scoring_request_ref = format!("scoring_request_{suffix}");
    let scoring_result_ref = format!("scoring_result_{suffix}");

    let mut ledger = ResponseLedger::new(&session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: &event_ref,
                client_event_ref: &client_event_ref,
                item_version_ref: &item_version_ref,
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let responses = ledger
        .freeze_as(SessionState::Completed, &response_snapshot_ref)
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &responses,
        ScoringRequestInput {
            scoring_request_ref: &scoring_request_ref,
            response_snapshot_ref: &response_snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_big_five_en_v1",
            scoring_version_ref: "scoring_big_five_v1",
            calibration_reference: "calibration_big_five_v1",
            norm_version_ref: Some("norm_big_five_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scored = ScoringResult::new(
        &scoring_result_ref,
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_extraversion", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap();

    ResultSnapshot::new(
        &request,
        &scored,
        ResultSnapshotInput {
            result_snapshot_ref: result_ref,
            participant_ref,
            narrative_version_ref: "narrative_big_five_v1",
            consent_snapshot_refs: &["consent_service_v1"],
            created_at_unix_ms: 1_700_000_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn personal_export(snapshot: &ResultSnapshot, export_ref: &str) -> ResultExport {
    ResultExport::from_snapshot(
        snapshot,
        ResultExportInput {
            export_ref,
            locale: "en-US",
            exported_at_unix_ms: 1_700_000_100_000,
            limitations: &[LIMITATION],
        },
    )
    .unwrap()
}

fn participant(participant_ref: &str, tenant_ref: &str) -> ParticipantRecord {
    ParticipantRecord::new_anonymous(participant_ref, tenant_ref, 1_699_999_000_000).unwrap()
}

fn actor(tenant_ref: &str, participant_ref: &str) -> AuthorizationContext {
    AuthorizationContext::new(
        tenant_ref,
        "subject_account_owner",
        Some(participant_ref),
        &[ProductRole::Participant],
    )
    .unwrap()
}

fn fixture() -> (AuthorizationContext, ParticipantRecord, ResultSnapshot, ResultExport) {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let export = personal_export(&snapshot, "result_export_alpha");
    (
        actor("tenant_alpha", "participant_alpha"),
        participant("participant_alpha", "tenant_alpha"),
        snapshot,
        export,
    )
}

#[test]
fn authorized_owner_gets_exact_machine_readable_export() {
    let (actor, participant, snapshot, export) = fixture();
    let response = handle_result_export_http_request(
        "GET /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\nAccept: application/json\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/json");
    assert_eq!(response.body(), export.json_document());
    assert_eq!(response.allow(), None);
}

#[test]
fn authorized_owner_can_request_the_human_readable_export() {
    let (actor, participant, snapshot, export) = fixture();
    let response = handle_result_export_http_request(
        "GET /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\nAccept: text/plain\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "text/plain; charset=utf-8");
    assert_eq!(response.body(), export.human_readable_report());
}

#[test]
fn missing_accept_defaults_to_machine_readable_export() {
    let (actor, participant, snapshot, export) = fixture();
    let response = handle_result_export_http_request(
        "GET /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\nHost: example.test\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );

    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/json");
}

#[test]
fn cross_tenant_denial_precedes_export_binding_details() {
    let (_, participant, snapshot, _) = fixture();
    let other_snapshot = result_snapshot("result_snapshot_beta", "participant_alpha", "beta");
    let wrong_export = personal_export(&other_snapshot, "result_export_beta");
    let actor = actor("tenant_other", "participant_alpha");
    let response = handle_result_export_http_request(
        "GET /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\nAccept: application/json\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &wrong_export,
    );

    assert_eq!(response.status(), 403);
    assert_eq!(response.content_type(), "application/problem+json");
    assert!(!response.body().contains("participant_alpha"));
    assert!(!response.body().contains("result_snapshot_beta"));
    assert!(!response.body().contains("result_export_beta"));
}

#[test]
fn authorized_owner_cannot_rebind_route_to_another_result_or_export() {
    let (actor, participant, snapshot, export) = fixture();
    for request in [
        "GET /v1/results/result_snapshot_other/exports/result_export_alpha HTTP/1.1\r\n\r\n",
        "GET /v1/results/result_snapshot_alpha/exports/result_export_other HTTP/1.1\r\n\r\n",
    ] {
        let response = handle_result_export_http_request(
            request,
            &actor,
            &participant,
            &snapshot,
            &export,
        );
        assert_eq!(response.status(), 404);
        assert!(!response.body().contains("result_snapshot_alpha"));
        assert!(!response.body().contains("result_export_alpha"));
    }
}

#[test]
fn unsupported_method_and_representation_are_explicit() {
    let (actor, participant, snapshot, export) = fixture();
    let method = handle_result_export_http_request(
        "POST /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );
    assert_eq!(method.status(), 405);
    assert_eq!(method.allow(), Some("GET"));

    let representation = handle_result_export_http_request(
        "GET /v1/results/result_snapshot_alpha/exports/result_export_alpha HTTP/1.1\r\nAccept: application/xml\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );
    assert_eq!(representation.status(), 406);
    assert_eq!(representation.content_type(), "application/problem+json");
}

#[test]
fn malformed_or_unknown_routes_fail_without_echoing_request_identity() {
    let (actor, participant, snapshot, export) = fixture();
    let malformed = handle_result_export_http_request(
        "GET only-two-parts\r\n\r\n",
        &actor,
        &participant,
        &snapshot,
        &export,
    );
    assert_eq!(malformed.status(), 400);

    for request in [
        "GET /v1/results/123/exports/result_export_alpha HTTP/1.1\r\n\r\n",
        "GET /v1/unknown/result_snapshot_alpha HTTP/1.1\r\n\r\n",
    ] {
        let response = handle_result_export_http_request(
            request,
            &actor,
            &participant,
            &snapshot,
            &export,
        );
        assert!(matches!(response.status(), 400 | 404));
        assert!(!response.body().contains("result_snapshot_alpha"));
    }
}
