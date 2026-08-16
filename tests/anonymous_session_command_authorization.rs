//! Contract tests for anonymous command authorization against loaded aggregates.
//!
//! A transport must load the participant and assessment session from the product store,
//! then ask this boundary whether the already-verified anonymous session may command
//! that exact loaded session. Callers do not invent the resource tenant or owner.

use psychometrics_commons_runtime::anonymous_authorization::{
    apply_anonymous_session_command, authorize_anonymous_session_command,
    AnonymousResourceAuthorizationError, AnonymousSessionCommandError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionState, TransitionErrorKind,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001"],
        RELEASE_DIGEST,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_11d5b1e7",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_20f6c2a8",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn participant(tenant_ref: &str, participant_ref: &str) -> ParticipantRecord {
    ParticipantRecord::new_anonymous(participant_ref, tenant_ref, 1_000).unwrap()
}

fn session(participant_ref: &str, session_ref: &str) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        participant_ref,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn anonymous_context(
    tenant_ref: &str,
    participant_ref: &str,
    session_ref: &str,
) -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        tenant_ref,
        participant_ref,
        session_ref,
        "anonymous_command_evidence_alpha",
        2_000,
    )
    .unwrap()
}

#[test]
fn current_anonymous_proof_may_command_only_its_loaded_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let loaded = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &loaded, 1_500),
        Ok(())
    );
}

#[test]
fn anonymous_command_authorization_uses_loaded_participant_tenant_not_caller_scope() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let foreign_owner = participant("tenant_beta", "participant_alpha");
    let loaded = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &foreign_owner, &loaded, 1_500),
        Err(AnonymousResourceAuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn anonymous_command_authorization_rejects_a_session_owned_by_another_loaded_participant() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &other_persons_session, 1_500),
        Err(AnonymousResourceAuthorizationError::OwnerMismatch)
    );
}

#[test]
fn anonymous_command_authorization_rejects_a_different_loaded_session_for_the_same_owner() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let other_session = session("participant_alpha", "session_beta");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &other_session, 1_500),
        Err(AnonymousResourceAuthorizationError::SessionMismatch)
    );
}

#[test]
fn anonymous_command_authorization_fails_closed_for_zero_or_expired_server_time() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let loaded = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &loaded, 0),
        Err(AnonymousResourceAuthorizationError::InvalidTimestamp)
    );
    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &loaded, 2_000),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
}

#[test]
fn anonymous_command_authorization_rejects_compound_failures_in_time_then_owner_order() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &other_persons_session, 0),
        Err(AnonymousResourceAuthorizationError::InvalidTimestamp)
    );
    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &other_persons_session, 2_000),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
}

#[test]
fn authorized_anonymous_proof_may_activate_only_its_loaded_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut loaded = session("participant_alpha", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut loaded,
            "command_activate_alpha",
            1,
            SessionCommand::Activate,
            1_500,
        ),
        Ok(SessionState::Active)
    );
    assert_eq!(loaded.state(), SessionState::Active);
}

#[test]
fn unauthorized_anonymous_command_does_not_mutate_the_loaded_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut other_session = session("participant_alpha", "session_beta");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut other_session,
            "command_activate_beta",
            1,
            SessionCommand::Activate,
            1_500,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::SessionMismatch
        ))
    );
    assert_eq!(other_session.state(), SessionState::Created);
}

#[test]
fn expired_anonymous_proof_cannot_apply_an_otherwise_legal_session_command() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut loaded = session("participant_alpha", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut loaded,
            "command_activate_expired",
            1,
            SessionCommand::Activate,
            2_000,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::Expired
        ))
    );
    assert_eq!(loaded.state(), SessionState::Created);
}

#[test]
fn authorized_anonymous_command_still_fails_closed_on_illegal_lifecycle_transition() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut loaded = session("participant_alpha", "session_alpha");

    let error = apply_anonymous_session_command(
        &actor,
        &owner,
        &mut loaded,
        "command_complete_too_early",
        1,
        SessionCommand::Complete,
        1_500,
    )
    .expect_err("Created sessions cannot complete");
    match error {
        AnonymousSessionCommandError::Transition(transition) => {
            assert_eq!(transition.state(), SessionState::Created);
            assert_eq!(transition.command(), SessionCommand::Complete);
            assert_eq!(transition.kind(), TransitionErrorKind::InvalidTransition);
        }
        other => panic!("expected lifecycle rejection, got {other:?}"),
    }
    assert_eq!(loaded.state(), SessionState::Created);
    assert!(error.to_string().contains("Complete"));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn anonymous_session_command_errors_preserve_safe_authorization_text() {
    let authorization = AnonymousSessionCommandError::Authorization(
        AnonymousResourceAuthorizationError::SessionMismatch,
    );
    assert_eq!(
        authorization.to_string(),
        "anonymous session authority does not match the resource session"
    );
    assert!(std::error::Error::source(&authorization).is_some());
}
