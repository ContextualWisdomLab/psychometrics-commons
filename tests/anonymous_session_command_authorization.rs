//! Contract tests for anonymous command authorization against supplied aggregates.
//!
//! A transport should hold participant and session records before calling this
//! boundary. These tests pass supplied records; the type system does not prove
//! they were loaded from the product store. Persist/reload of live measurement sessions is implemented. Append-only identity-link history persist remains a later slice. This gate still does not prove store load.

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
/// Session starts after the Big Five Korean release is published at `10_200`.
const SESSION_CREATED_AT_UNIX_MS: u64 = 10_300;
/// Trusted now sits after session creation and before exclusive proof expiry.
const COMMAND_NOW_UNIX_MS: u64 = 11_000;
/// Exclusive proof expiry: valid at `11_999`, expired at `12_000`.
const PROOF_VALID_UNTIL_UNIX_MS: u64 = 12_000;

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
        SESSION_CREATED_AT_UNIX_MS,
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
        PROOF_VALID_UNTIL_UNIX_MS,
    )
    .unwrap()
}

#[test]
fn current_anonymous_proof_may_command_only_its_supplied_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &supplied, COMMAND_NOW_UNIX_MS),
        Ok(())
    );
}

#[test]
fn anonymous_command_authorization_uses_supplied_participant_tenant_not_caller_scope() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let foreign_owner = participant("tenant_beta", "participant_alpha");
    let supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &foreign_owner, &supplied, COMMAND_NOW_UNIX_MS),
        Err(AnonymousResourceAuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn anonymous_command_authorization_rejects_a_session_owned_by_another_supplied_participant() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(
            &actor,
            &owner,
            &other_persons_session,
            COMMAND_NOW_UNIX_MS
        ),
        Err(AnonymousResourceAuthorizationError::OwnerMismatch)
    );
}

#[test]
fn anonymous_command_authorization_rejects_a_different_supplied_session_for_the_same_owner() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let other_session = session("participant_alpha", "session_beta");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &other_session, COMMAND_NOW_UNIX_MS),
        Err(AnonymousResourceAuthorizationError::SessionMismatch)
    );
}

#[test]
fn anonymous_command_authorization_fails_closed_for_zero_or_expired_server_time() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &supplied, 0),
        Err(AnonymousResourceAuthorizationError::InvalidTimestamp)
    );
    assert_eq!(
        authorize_anonymous_session_command(&actor, &owner, &supplied, PROOF_VALID_UNTIL_UNIX_MS),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
    assert_eq!(
        authorize_anonymous_session_command(
            &actor,
            &owner,
            &supplied,
            PROOF_VALID_UNTIL_UNIX_MS + 1
        ),
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
        authorize_anonymous_session_command(
            &actor,
            &owner,
            &other_persons_session,
            PROOF_VALID_UNTIL_UNIX_MS
        ),
        Err(AnonymousResourceAuthorizationError::Expired)
    );
}

#[test]
fn anonymous_command_authorization_rejects_actor_when_supplied_participant_and_session_agree() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let other_owner = participant("tenant_alpha", "participant_beta");
    let other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(
            &actor,
            &other_owner,
            &other_persons_session,
            COMMAND_NOW_UNIX_MS
        ),
        Err(AnonymousResourceAuthorizationError::OwnerMismatch)
    );
}

#[test]
fn anonymous_command_authorization_rejects_compound_foreign_tenant_and_inconsistent_supplied_pair_as_cross_tenant(
) {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let foreign_owner = participant("tenant_beta", "participant_alpha");
    let other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        authorize_anonymous_session_command(
            &actor,
            &foreign_owner,
            &other_persons_session,
            COMMAND_NOW_UNIX_MS
        ),
        Err(AnonymousResourceAuthorizationError::CrossTenantDenied)
    );
}

#[test]
fn authorized_anonymous_proof_may_activate_only_its_supplied_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut supplied,
            "command_activate_alpha",
            1,
            SessionCommand::Activate,
            COMMAND_NOW_UNIX_MS,
        ),
        Ok(SessionState::Active)
    );
    assert_eq!(supplied.state(), SessionState::Active);
}

#[test]
fn unauthorized_anonymous_command_does_not_mutate_the_supplied_session() {
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
            COMMAND_NOW_UNIX_MS,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::SessionMismatch
        ))
    );
    assert_eq!(other_session.state(), SessionState::Created);
}

#[test]
fn cross_tenant_anonymous_command_does_not_mutate_the_supplied_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let foreign_owner = participant("tenant_beta", "participant_alpha");
    let mut supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &foreign_owner,
            &mut supplied,
            "command_activate_foreign_tenant",
            1,
            SessionCommand::Activate,
            COMMAND_NOW_UNIX_MS,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::CrossTenantDenied
        ))
    );
    assert_eq!(supplied.state(), SessionState::Created);
}

#[test]
fn owner_mismatch_anonymous_command_does_not_mutate_the_supplied_session() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut other_persons_session = session("participant_beta", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut other_persons_session,
            "command_activate_foreign_owner",
            1,
            SessionCommand::Activate,
            COMMAND_NOW_UNIX_MS,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::OwnerMismatch
        ))
    );
    assert_eq!(other_persons_session.state(), SessionState::Created);
}

#[test]
fn expired_anonymous_proof_cannot_apply_an_otherwise_legal_session_command() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut supplied = session("participant_alpha", "session_alpha");

    assert_eq!(
        apply_anonymous_session_command(
            &actor,
            &owner,
            &mut supplied,
            "command_activate_expired",
            1,
            SessionCommand::Activate,
            PROOF_VALID_UNTIL_UNIX_MS,
        ),
        Err(AnonymousSessionCommandError::Authorization(
            AnonymousResourceAuthorizationError::Expired
        ))
    );
    assert_eq!(supplied.state(), SessionState::Created);
}

#[test]
fn authorized_anonymous_command_still_fails_closed_on_illegal_lifecycle_transition() {
    let actor = anonymous_context("tenant_alpha", "participant_alpha", "session_alpha");
    let owner = participant("tenant_alpha", "participant_alpha");
    let mut supplied = session("participant_alpha", "session_alpha");

    let error = apply_anonymous_session_command(
        &actor,
        &owner,
        &mut supplied,
        "command_complete_too_early",
        1,
        SessionCommand::Complete,
        COMMAND_NOW_UNIX_MS,
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
    assert_eq!(supplied.state(), SessionState::Created);
    assert!(error.to_string().contains("Complete"));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn anonymous_session_command_authorization_errors_display_and_source_authorization_variants() {
    let cases = [
        (
            AnonymousResourceAuthorizationError::InvalidTimestamp,
            "anonymous resource authorization requires positive server time",
        ),
        (
            AnonymousResourceAuthorizationError::Expired,
            "anonymous session authority is expired",
        ),
        (
            AnonymousResourceAuthorizationError::CrossTenantDenied,
            "anonymous session authority does not match the resource tenant",
        ),
        (
            AnonymousResourceAuthorizationError::ResourceKindMismatch,
            "anonymous session authority is limited to its assessment-session resource",
        ),
        (
            AnonymousResourceAuthorizationError::OwnerMismatch,
            "anonymous session authority does not match the resource participant",
        ),
        (
            AnonymousResourceAuthorizationError::SessionMismatch,
            "anonymous session authority does not match the resource session",
        ),
    ];

    for (inner, expected) in cases {
        let error = AnonymousSessionCommandError::Authorization(inner);
        assert_eq!(error.to_string(), expected);
        assert!(std::error::Error::source(&error).is_some());
    }
}
