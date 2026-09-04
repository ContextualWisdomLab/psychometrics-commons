//! Session-command writes require server-verified participant authority.
//!
//! Possession of an opaque session reference is not authorization. The public
//! authority-free handler must fail closed without changing lifecycle state,
//! while an embedding host may supply a server-owned participant record and an
//! authenticated product context or current anonymous-session credential for
//! that exact participant/session resource.

use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::session::{AssessmentSession, SessionState};
use psychometrics_commons_runtime::session_command_http::{
    handle_authorized_session_command_http_request, handle_session_command_http_request,
    SessionCommandAuthority, SessionCommandHttpRuntime,
};

const TENANT_REF: &str = "tenant_session_command_http_contract";
const PARTICIPANT_REF: &str = "ptc_command_authorization_owner";
const SESSION_REF: &str = "ses_command_authorization_target";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PROOF_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_command_authorization_v1",
        "instrument_big_five",
        "instrument_version_command_authorization_v1",
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
        "publication_evidence_command_authorization_v1",
        "evidence_policy_self_reflection_v1",
        "release_command_authorization_v1",
        "instrument_version_command_authorization_v1",
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
            "publication_review_command_authorization",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_command_authorization",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn created_session() -> AssessmentSession {
    AssessmentSession::new(
        SESSION_REF,
        PARTICIPANT_REF,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous(PARTICIPANT_REF, TENANT_REF, 19_000).unwrap()
}

fn owner_actor() -> AuthorizationContext {
    AuthorizationContext::new(
        TENANT_REF,
        "subject_session_command_owner",
        Some(PARTICIPANT_REF),
        &[ProductRole::Participant],
    )
    .unwrap()
}

fn activate_request() -> String {
    let body = "{\"command\":\"activate\"}";
    format!(
        "POST /v1/sessions/{SESSION_REF}/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: cmd_authorized_activate\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn unsafe_idempotency_request() -> String {
    let body = "{\"command\":\"activate\"}";
    format!(
        "POST /v1/sessions/{SESSION_REF}/commands HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: cmd\u{200b}_authorized_activate\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn authority_free_handler_cannot_mutate_a_known_session() {
    let mut runtime = SessionCommandHttpRuntime::new(vec![created_session()]);

    let response = handle_session_command_http_request(&activate_request(), &mut runtime);

    assert_eq!(response.status(), 404);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Created
    );
}

#[test]
fn authenticated_owner_can_command_the_exact_server_owned_session() {
    let mut runtime = SessionCommandHttpRuntime::new(vec![created_session()]);
    let participant = participant();
    let actor = owner_actor();
    let authority = SessionCommandAuthority::Authenticated(&actor);

    let response = handle_authorized_session_command_http_request(
        &activate_request(),
        &authority,
        &participant,
        &mut runtime,
    );

    assert_eq!(response.status(), 200);
    assert!(response.body().contains("\"state\":\"active\""));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Active
    );
}

#[test]
fn unsafe_idempotency_key_is_rejected_before_session_or_authority_lookup() {
    let request = unsafe_idempotency_request();
    let mut public_runtime = SessionCommandHttpRuntime::new(vec![created_session()]);
    let public = handle_session_command_http_request(&request, &mut public_runtime);

    let mut authorized_runtime = SessionCommandHttpRuntime::new(vec![created_session()]);
    let participant = participant();
    let actor = owner_actor();
    let authority = SessionCommandAuthority::Authenticated(&actor);
    let authorized = handle_authorized_session_command_http_request(
        &request,
        &authority,
        &participant,
        &mut authorized_runtime,
    );

    assert_eq!(public.status(), 400);
    assert_eq!(authorized.status(), 400);
    assert_eq!(public.body(), authorized.body());
    assert!(public
        .body()
        .contains("urn:psychometrics-commons:problem:missing-idempotency-key"));
    assert_eq!(
        public_runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Created
    );
    assert_eq!(
        authorized_runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Created
    );
}

#[test]
fn foreign_authenticated_participant_cannot_probe_or_mutate_the_session() {
    let mut runtime = SessionCommandHttpRuntime::new(vec![created_session()]);
    let participant = participant();
    let foreign_actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_session_command_foreign",
        Some("ptc_command_authorization_foreign"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = SessionCommandAuthority::Authenticated(&foreign_actor);

    let response = handle_authorized_session_command_http_request(
        &activate_request(),
        &authority,
        &participant,
        &mut runtime,
    );

    assert_eq!(response.status(), 404);
    assert!(response
        .body()
        .contains("urn:psychometrics-commons:problem:session-not-found"));
    assert_eq!(
        runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Created
    );
}

#[test]
fn current_anonymous_credential_can_command_but_revocation_fails_closed() {
    let participant = participant();
    let mut credential = AnonymousCredential::new(
        "credential_session_command_authorization",
        TENANT_REF,
        PARTICIPANT_REF,
        SESSION_REF,
        PROOF_DIGEST,
        19_000,
        30_000,
    )
    .unwrap();
    let context = credential
        .session_context(
            PROOF_DIGEST,
            TENANT_REF,
            PARTICIPANT_REF,
            SESSION_REF,
            20_100,
        )
        .unwrap();
    let allowed_authority = SessionCommandAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 20_100,
    };
    let mut allowed_runtime = SessionCommandHttpRuntime::new(vec![created_session()]);

    let allowed = handle_authorized_session_command_http_request(
        &activate_request(),
        &allowed_authority,
        &participant,
        &mut allowed_runtime,
    );
    assert_eq!(allowed.status(), 200);
    assert_eq!(
        allowed_runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Active
    );

    credential.revoke(20_200).unwrap();
    let revoked_authority = SessionCommandAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 20_300,
    };
    let mut revoked_runtime = SessionCommandHttpRuntime::new(vec![created_session()]);
    let revoked = handle_authorized_session_command_http_request(
        &activate_request(),
        &revoked_authority,
        &participant,
        &mut revoked_runtime,
    );

    assert_eq!(revoked.status(), 404);
    assert_eq!(
        revoked_runtime.session(SESSION_REF).unwrap().state(),
        SessionState::Created
    );
}
