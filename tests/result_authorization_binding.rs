//! Regression tests for result reads bound to authoritative participant ownership.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_authorization::authorize_result_read;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;
use response_support::{active_session, advance_to};

const ENGINE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn result_snapshot(participant_ref: &str) -> ResultSnapshot {
    let mut session = active_session("session_alpha");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    ledger
        .record(
            &session,
            ResponseWrite {
                server_event_ref: "response_event_alpha",
                client_event_ref: "client_event_alpha",
                item_version_ref: "item_version_alpha",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    advance_to(&mut session, SessionState::Completed);
    let response_snapshot = ledger
        .freeze_as(&session, "response_snapshot_alpha")
        .unwrap();
    let scoring_request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_alpha",
            response_snapshot_ref: "response_snapshot_alpha",
            assessment_spec_ref: "assessment_spec_alpha",
            instrument_version_ref: "instrument_version_alpha",
            scoring_version_ref: "scoring_version_alpha",
            calibration_reference: "calibration_reference_alpha",
            norm_version_ref: Some("norm_version_alpha"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_alpha",
        &scoring_request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap();

    ResultSnapshot::new(
        &scoring_request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_alpha",
            participant_ref,
            narrative_version_ref: "narrative_version_alpha",
            consent_snapshot_refs: &["service_consent_alpha"],
            created_at_unix_ms: 1_786_240_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn participant_actor(tenant_ref: &str, participant_ref: Option<&str>) -> AuthorizationContext {
    AuthorizationContext::new(
        tenant_ref,
        "subject_alpha",
        participant_ref,
        &[ProductRole::Participant],
    )
    .unwrap()
}

#[test]
fn result_read_uses_the_tenant_and_owner_from_authoritative_domain_records() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1).unwrap();
    let result = result_snapshot("participant_alpha");
    let actor = participant_actor("tenant_alpha", Some("participant_alpha"));

    assert_eq!(authorize_result_read(&actor, &participant, &result), Ok(()));
}

#[test]
fn result_read_rejects_a_participant_record_that_does_not_own_the_result() {
    let participant =
        ParticipantRecord::new_anonymous("participant_beta", "tenant_alpha", 1).unwrap();
    let result = result_snapshot("participant_alpha");
    let actor = participant_actor("tenant_alpha", Some("participant_beta"));

    assert_eq!(
        authorize_result_read(&actor, &participant, &result),
        Err(AuthorizationError::OwnerMismatch)
    );
}

#[test]
fn result_read_rejects_cross_tenant_actor_context() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1).unwrap();
    let result = result_snapshot("participant_alpha");
    let actor = participant_actor("tenant_beta", Some("participant_alpha"));

    assert_eq!(
        authorize_result_read(&actor, &participant, &result),
        Err(AuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn result_read_rejects_a_different_authenticated_participant() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1).unwrap();
    let result = result_snapshot("participant_alpha");
    let actor = participant_actor("tenant_alpha", Some("participant_beta"));

    assert_eq!(
        authorize_result_read(&actor, &participant, &result),
        Err(AuthorizationError::OwnerMismatch)
    );
}

#[test]
fn result_read_requires_an_operational_participant_identity() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1).unwrap();
    let result = result_snapshot("participant_alpha");
    let actor = participant_actor("tenant_alpha", None);

    assert_eq!(
        authorize_result_read(&actor, &participant, &result),
        Err(AuthorizationError::ParticipantIdentityRequired)
    );
}
