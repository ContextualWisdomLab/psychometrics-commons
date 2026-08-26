//! Personal result exports inherit the stored result's tenant and owner authorization.
//!
//! Export generation intentionally keeps the participant reference so an authorized
//! participant can use their own data. Before an adapter returns that export, it must
//! authorize the stored result and prove that the export was derived from that exact
//! immutable result rather than trusting caller-supplied tenant or owner fields.

use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{ResultExport, ResultExportInput};
use psychometrics_commons_runtime::result_export_authorization::{
    authorize_result_export_read, ResultExportAuthorizationError,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

#[path = "response_support/mod.rs"]
mod response_support;
use std::error::Error;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn result_snapshot(result_ref: &str, participant_ref: &str, suffix: &str) -> ResultSnapshot {
    let session_ref = format!("session_{suffix}");
    let event_ref = format!("event_{suffix}");
    let client_event_ref = format!("client_{suffix}");
    let item_version_ref = format!("item_{suffix}");
    let response_snapshot_ref = format!("response_snapshot_{suffix}");
    let scoring_request_ref = format!("scoring_request_{suffix}");
    let scoring_result_ref = format!("scoring_result_{suffix}");

    let mut session = response_support::active_session(&session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    ledger
        .record(
            &session,
            ResponseWrite {
                server_event_ref: &event_ref,
                client_event_ref: &client_event_ref,
                item_version_ref: &item_version_ref,
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    response_support::advance_to(&mut session, SessionState::Completed);
    let responses = ledger.freeze_as(&session, &response_snapshot_ref).unwrap();
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
            limitations: &["This result is not a diagnosis or employment-fitness decision."],
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

#[test]
fn owner_can_read_export_bound_to_the_exact_stored_result() {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let export = personal_export(&snapshot, "result_export_alpha");
    let participant = participant("participant_alpha", "tenant_alpha");
    let actor = actor("tenant_alpha", "participant_alpha");

    assert_eq!(
        authorize_result_export_read(&actor, &participant, &snapshot, &export),
        Ok(())
    );
}

#[test]
fn cross_tenant_actor_is_denied_before_export_binding_is_disclosed() {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let other_snapshot = result_snapshot("result_snapshot_beta", "participant_alpha", "beta");
    let wrong_export = personal_export(&other_snapshot, "result_export_beta");
    let participant = participant("participant_alpha", "tenant_alpha");
    let actor = actor("tenant_other", "participant_alpha");

    assert_eq!(
        authorize_result_export_read(&actor, &participant, &snapshot, &wrong_export),
        Err(ResultExportAuthorizationError::Authorization(
            AuthorizationError::CrossTenantDenied
        ))
    );
}

#[test]
fn authorized_actor_cannot_receive_an_export_from_another_result() {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let other_snapshot = result_snapshot("result_snapshot_beta", "participant_alpha", "beta");
    let wrong_export = personal_export(&other_snapshot, "result_export_beta");
    let participant = participant("participant_alpha", "tenant_alpha");
    let actor = actor("tenant_alpha", "participant_alpha");

    assert_eq!(
        authorize_result_export_read(&actor, &participant, &snapshot, &wrong_export),
        Err(ResultExportAuthorizationError::ExportBindingMismatch)
    );
}

#[test]
fn authorized_actor_rejects_same_result_reference_with_another_export_owner() {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let other_owner_snapshot =
        result_snapshot("result_snapshot_alpha", "participant_other", "other_owner");
    let wrong_export = personal_export(&other_owner_snapshot, "result_export_other_owner");
    let participant = participant("participant_alpha", "tenant_alpha");
    let actor = actor("tenant_alpha", "participant_alpha");

    assert_eq!(
        authorize_result_export_read(&actor, &participant, &snapshot, &wrong_export),
        Err(ResultExportAuthorizationError::ExportBindingMismatch)
    );
}

#[test]
fn participant_record_must_own_the_result_before_export_access() {
    let snapshot = result_snapshot("result_snapshot_alpha", "participant_alpha", "alpha");
    let export = personal_export(&snapshot, "result_export_alpha");
    let wrong_participant = participant("participant_other", "tenant_alpha");
    let actor = actor("tenant_alpha", "participant_alpha");

    assert_eq!(
        authorize_result_export_read(&actor, &wrong_participant, &snapshot, &export),
        Err(ResultExportAuthorizationError::Authorization(
            AuthorizationError::OwnerMismatch
        ))
    );
}

#[test]
fn export_authorization_errors_keep_safe_operator_text_and_sources() {
    let authorization =
        ResultExportAuthorizationError::Authorization(AuthorizationError::CrossTenantDenied);
    assert_eq!(
        authorization.to_string(),
        "resource tenant does not match the authenticated tenant"
    );
    assert_eq!(
        authorization.source().map(ToString::to_string).as_deref(),
        Some("resource tenant does not match the authenticated tenant")
    );

    let binding = ResultExportAuthorizationError::ExportBindingMismatch;
    assert_eq!(
        binding.to_string(),
        "personal result export does not belong to the authorized immutable result"
    );
    assert!(binding.source().is_none());
}
