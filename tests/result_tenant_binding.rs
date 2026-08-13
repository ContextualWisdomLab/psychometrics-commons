//! Result snapshots remain bound to the tenant that owns the participant resource.
//!
//! A participant/result reference is not a tenant authority by itself. Persisted and served result
//! state needs an explicit product-owned tenant reference so a valid resource identifier cannot be
//! replayed under an implicit/default tenant. This is domain provenance only; authorization still
//! derives tenant context from authenticated product authority at the transport boundary.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn request_and_result() -> (ScoringRequest, ScoringResult) {
    let mut ledger = ResponseLedger::new("session_tenant_binding").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_tenant_binding",
                client_event_ref: "client_event_tenant_binding",
                item_version_ref: "item_version_tenant_binding",
                payload_digest: "sha256:response_tenant_binding",
            },
        )
        .unwrap();
    let response_snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_tenant_binding")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_tenant_binding",
            response_snapshot_ref: "response_snapshot_tenant_binding",
            assessment_spec_ref: "assessment_spec_tenant_binding",
            instrument_version_ref: "instrument_version_tenant_binding",
            scoring_version_ref: "scoring_version_tenant_binding",
            calibration_reference: "calibration_tenant_binding",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_tenant_binding",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 0.4, Some(0.1)).unwrap()],
    )
    .unwrap();
    (request, result)
}

fn input<'a>(tenant_ref: &'a str) -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_snapshot_tenant_binding",
        tenant_ref,
        participant_ref: "participant_tenant_binding",
        narrative_version_ref: "narrative_tenant_binding",
        consent_snapshot_refs: &["consent_tenant_binding"],
        created_at_unix_ms: 1_786_500_000_000,
        supersedes_ref: None,
    }
}

#[test]
fn result_snapshot_preserves_explicit_tenant_provenance() {
    let (request, result) = request_and_result();
    let snapshot = ResultSnapshot::new(&request, &result, input(" tenant_alpha ")).unwrap();

    assert_eq!(snapshot.tenant_ref(), "tenant_alpha");
    assert_eq!(snapshot.participant_ref(), "participant_tenant_binding");
}

#[test]
fn result_snapshot_rejects_missing_or_numeric_tenant_identity() {
    let (request, result) = request_and_result();

    for invalid_tenant in ["", "   ", "42"] {
        assert_eq!(
            ResultSnapshot::new(&request, &result, input(invalid_tenant)).unwrap_err(),
            ResultSnapshotError::EmptyReference
        );
    }
}
