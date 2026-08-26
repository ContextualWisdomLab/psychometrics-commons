//! Response writes require server-verified participant/session authority.
//!
//! The public HTTP application boundary consumes authority that the embedding host
//! has already verified. It never trusts route identity or caller-supplied ownership
//! to decide who may mutate an assessment session. Authenticated and anonymous
//! participants both remain supported; missing/foreign authority is intentionally
//! indistinguishable from an unknown session to avoid a session-existence oracle.

use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::response_http::{
    handle_authorized_response_http_request, handle_response_http_request, ResponseHttpRuntime,
    ResponseWriteAuthority,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand};

const SESSION_REF: &str = "ses_response_authority_alpha";
const PARTICIPANT_REF: &str = "ptc_response_authority_alpha";
const TENANT_REF: &str = "tenant_response_authority_alpha";
const ITEM_REF: &str = "item_version_response_authority_001";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROOF_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_response_authority_v1",
        "instrument_response_authority",
        "instrument_version_response_authority_v1",
        "construct_response_authority",
        &[ITEM_REF],
        "ko-KR",
        "assessment_spec_response_authority_v1",
        "scoring_version_response_authority_v1",
        "calibration_response_authority_v1",
        None,
        "narrative_response_authority_v1",
        &["consent_service_response_authority_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_response_authority_v1",
        "evidence_policy_response_authority_v1",
        "release_response_authority_v1",
        "instrument_version_response_authority_v1",
        &[ITEM_REF],
        RELEASE_DIGEST,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_response_authority_v1",
        "scoring_version_response_authority_v1",
        "calibration_response_authority_v1",
        None,
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_response_authority_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_response_authority_v1"],
        &["recovery_response_authority_v1"],
        &["approval_response_authority_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_response_authority",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_response_authority",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous(PARTICIPANT_REF, TENANT_REF, 10_300).unwrap()
}

fn active_session(release: &InstrumentRelease) -> AssessmentSession {
    let mut session = AssessmentSession::new(
        SESSION_REF,
        PARTICIPANT_REF,
        release,
        "ko-KR",
        10_400,
    )
    .unwrap();
    session
        .apply_command("cmd_activate_response_authority", 1, SessionCommand::Activate)
        .unwrap();
    session
}

fn runtime() -> ResponseHttpRuntime {
    let release = release();
    ResponseHttpRuntime::new(
        vec![active_session(&release)],
        vec![release],
        "evt_response_authority_001",
    )
}

fn request(session_ref: &str) -> String {
    let body = format!(
        "{{\"item_version_ref\":\"{ITEM_REF}\",\"payload_digest\":\"{PAYLOAD_DIGEST}\"}}"
    );
    format!(
        "POST /v1/sessions/{session_ref}/responses HTTP/1.1\r\nHost: localhost\r\nIdempotency-Key: idem_response_authority_001\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn unauthenticated_and_foreign_authenticated_callers_cannot_probe_session_existence() {
    let mut unauthenticated_runtime = runtime();
    let unauthenticated =
        handle_response_http_request(&request(SESSION_REF), &mut unauthenticated_runtime);
    assert_eq!(unauthenticated.status(), 404);
    assert_eq!(unauthenticated_runtime.event_count(SESSION_REF), 0);

    let participant = participant();
    let foreign_actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_response_authority_foreign",
        Some("ptc_response_authority_foreign"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = ResponseWriteAuthority::Authenticated(&foreign_actor);
    let mut existing_runtime = runtime();
    let existing = handle_authorized_response_http_request(
        &request(SESSION_REF),
        &authority,
        &participant,
        &mut existing_runtime,
    );
    let mut missing_runtime = runtime();
    let missing = handle_authorized_response_http_request(
        &request("ses_response_authority_missing"),
        &authority,
        &participant,
        &mut missing_runtime,
    );

    assert_eq!(existing.status(), 404);
    assert_eq!(existing, missing);
    assert_eq!(existing_runtime.event_count(SESSION_REF), 0);
}

#[test]
fn authenticated_owner_may_record_on_exact_owned_session() {
    let participant = participant();
    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_response_authority_owner",
        Some(PARTICIPANT_REF),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = ResponseWriteAuthority::Authenticated(&actor);
    let mut runtime = runtime();

    let response = handle_authorized_response_http_request(
        &request(SESSION_REF),
        &authority,
        &participant,
        &mut runtime,
    );

    assert_eq!(response.status(), 201);
    assert_eq!(runtime.event_count(SESSION_REF), 1);
}

#[test]
fn current_anonymous_credential_may_record_but_revocation_fails_closed() {
    let participant = participant();
    let mut credential = AnonymousCredential::new(
        "credential_response_authority_alpha",
        TENANT_REF,
        PARTICIPANT_REF,
        SESSION_REF,
        PROOF_DIGEST,
        10_300,
        20_000,
    )
    .unwrap();
    let context = credential
        .session_context(
            PROOF_DIGEST,
            TENANT_REF,
            PARTICIPANT_REF,
            SESSION_REF,
            10_500,
        )
        .unwrap();
    let mut allowed_runtime = runtime();
    let allowed_authority = ResponseWriteAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 10_500,
    };
    let allowed = handle_authorized_response_http_request(
        &request(SESSION_REF),
        &allowed_authority,
        &participant,
        &mut allowed_runtime,
    );
    assert_eq!(allowed.status(), 201);

    credential.revoke(10_600).unwrap();
    let revoked_authority = ResponseWriteAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 10_700,
    };
    let mut revoked_runtime = runtime();
    let revoked = handle_authorized_response_http_request(
        &request(SESSION_REF),
        &revoked_authority,
        &participant,
        &mut revoked_runtime,
    );
    assert_eq!(revoked.status(), 404);
    assert_eq!(revoked_runtime.event_count(SESSION_REF), 0);
}
