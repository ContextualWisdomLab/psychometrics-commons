//! Personal result-export representation negotiation must handle ordinary HTTP Accept syntax.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{ResultExport, ResultExportInput};
use psychometrics_commons_runtime::result_export_http::handle_result_export_http_request;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};

use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn fixture() -> (
    AuthorizationContext,
    ParticipantRecord,
    ResultSnapshot,
    ResultExport,
) {
    let participant_ref = "participant_accept_alpha";
    let session_ref = "session_accept_alpha";
    let response_snapshot_ref = "response_snapshot_accept_alpha";
    let response_snapshot = frozen_snapshot(
        session_ref,
        response_snapshot_ref,
        &[ResponseWrite {
            server_event_ref: "response_event_accept_alpha",
            client_event_ref: "client_event_accept_alpha",
            item_version_ref: "item_version_001",
            payload_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    let scoring_request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_accept_alpha",
            response_snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_accept_alpha",
        &scoring_request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_extraversion", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap();
    let result = ResultSnapshot::new(
        &scoring_request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_accept_alpha",
            participant_ref,
            narrative_version_ref: "narrative_big_five_v1",
            consent_snapshot_refs: &["consent_service_v1"],
            created_at_unix_ms: 30_000,
            supersedes_ref: None,
        },
    )
    .unwrap();
    let export = ResultExport::from_snapshot(
        &result,
        ResultExportInput {
            export_ref: "result_export_accept_alpha",
            locale: "en-US",
            exported_at_unix_ms: 31_000,
            limitations: &["This result is not a diagnosis or employment-fitness decision."],
        },
    )
    .unwrap();
    let actor = AuthorizationContext::new(
        "tenant_accept_alpha",
        "subject_accept_alpha",
        Some(participant_ref),
        &[ProductRole::Participant],
    )
    .unwrap();
    let participant =
        ParticipantRecord::new_anonymous(participant_ref, "tenant_accept_alpha", 20_000).unwrap();
    (actor, participant, result, export)
}

fn request(accept: &str) -> String {
    format!(
        "POST /v1/results/result_snapshot_accept_alpha/exports HTTP/1.1\r\nIdempotency-Key: result_export_accept_alpha\r\nAccept: {accept}\r\n\r\n"
    )
}

#[test]
fn accepts_parameterized_and_multi_value_supported_ranges() {
    let (actor, participant, result, export) = fixture();

    let text = handle_result_export_http_request(
        &request("text/plain; charset=utf-8"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(text.status(), 200);
    assert_eq!(text.content_type(), "text/plain; charset=utf-8");

    let json = handle_result_export_http_request(
        &request("application/xml, application/json"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(json.status(), 200);
    assert_eq!(json.content_type(), "application/json");
}

#[test]
fn honors_quality_weights_and_rejects_zero_or_malformed_supported_ranges() {
    let (actor, participant, result, export) = fixture();

    let text = handle_result_export_http_request(
        &request("application/json;q=0, text/plain;q=0.7"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(text.status(), 200);
    assert_eq!(text.content_type(), "text/plain; charset=utf-8");

    let none = handle_result_export_http_request(
        &request("application/json;q=0, text/plain;q=0"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(none.status(), 406);

    let malformed = handle_result_export_http_request(
        &request("application/json;q=bogus"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(malformed.status(), 406);
}

#[test]
fn specific_media_ranges_override_broader_wildcard_quality_for_each_representation() {
    let (actor, participant, result, export) = fixture();

    let json = handle_result_export_http_request(
        &request("text/plain;q=0.1, text/*;q=0.9, application/json;q=0.5"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(json.status(), 200);
    assert_eq!(json.content_type(), "application/json");

    let text = handle_result_export_http_request(
        &request("application/json;q=0.1, */*;q=0.9"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(text.status(), 200);
    assert_eq!(text.content_type(), "text/plain; charset=utf-8");

    let excluded_text = handle_result_export_http_request(
        &request("text/plain;q=0, */*;q=1"),
        &actor,
        &participant,
        &result,
        &export,
    );
    assert_eq!(excluded_text.status(), 200);
    assert_eq!(excluded_text.content_type(), "application/json");
}
