//! Created-session reconstitution without a currently published release.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionCreationError, SessionReconstitutionError,
    SessionState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const SESSION_REF: &str = "ses_9c2e1a0b4d5f67890123456789abcdef";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE_REF: &str = "release_big_five_ko_v1";
const VERSION_REF: &str = "instrument_version_big_five_ko_v1";

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        RELEASE_REF,
        "instrument_big_five",
        VERSION_REF,
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        VALID_DIGEST,
    )
    .unwrap()
}

fn approved_evidence() -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        RELEASE_REF,
        VERSION_REF,
        &["item_version_001", "item_version_002"],
        VALID_DIGEST,
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
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_f9f86084",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence())
        .unwrap();
    release
        .apply_command(
            "publication_publish_635a7491",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

#[test]
fn persisted_created_identity_restores_without_a_live_release() {
    let mut session = AssessmentSession::from_persisted_created(
        SESSION_REF,
        PARTICIPANT_REF,
        RELEASE_REF,
        VERSION_REF,
        VALID_DIGEST,
        "ko-KR",
        20_000,
    )
    .unwrap();

    assert_eq!(session.session_ref(), SESSION_REF);
    assert_eq!(session.participant_ref(), PARTICIPANT_REF);
    assert_eq!(session.instrument_release_ref(), RELEASE_REF);
    assert_eq!(session.instrument_version_ref(), VERSION_REF);
    assert_eq!(session.instrument_release_content_digest(), VALID_DIGEST);
    assert_eq!(session.locale(), "ko-KR");
    assert_eq!(session.created_at_unix_ms(), 20_000);
    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(
        session
            .apply_command(
                "cmd_activate_persisted_session",
                1,
                SessionCommand::Activate
            )
            .unwrap(),
        SessionState::Active
    );
}

#[test]
fn persisted_created_identity_survives_later_release_suspend_and_retire() {
    let mut release = published_release();
    let created =
        AssessmentSession::new(SESSION_REF, PARTICIPANT_REF, &release, "ko-KR", 20_000).unwrap();

    release
        .apply_command(
            "publication_suspend_after_create",
            PublicationCommand::Suspend,
            20_100,
        )
        .unwrap();
    assert_eq!(
        AssessmentSession::new(
            "ses_new_after_suspend_must_fail",
            PARTICIPANT_REF,
            &release,
            "ko-KR",
            20_200,
        ),
        Err(SessionCreationError::InstrumentReleaseUnavailable)
    );

    let mut restored = AssessmentSession::from_persisted_created(
        created.session_ref(),
        created.participant_ref(),
        created.instrument_release_ref(),
        created.instrument_version_ref(),
        created.instrument_release_content_digest(),
        created.locale(),
        created.created_at_unix_ms(),
    )
    .unwrap();
    assert_eq!(restored.instrument_release_ref(), RELEASE_REF);
    assert_eq!(restored.instrument_release_content_digest(), VALID_DIGEST);
    assert_eq!(restored.locale(), "ko-KR");
    assert_eq!(restored.state(), SessionState::Created);
    assert_eq!(
        restored
            .apply_command("cmd_activate_after_suspend", 1, SessionCommand::Activate)
            .unwrap(),
        SessionState::Active
    );

    release
        .apply_command(
            "publication_retire_after_suspend",
            PublicationCommand::Retire,
            20_300,
        )
        .unwrap();
    assert_eq!(
        AssessmentSession::new(
            "ses_new_after_retire_must_fail",
            PARTICIPANT_REF,
            &release,
            "ko-KR",
            20_400,
        ),
        Err(SessionCreationError::InstrumentReleaseUnavailable)
    );
    let retired_restore = AssessmentSession::from_persisted_created(
        created.session_ref(),
        created.participant_ref(),
        created.instrument_release_ref(),
        created.instrument_version_ref(),
        created.instrument_release_content_digest(),
        created.locale(),
        created.created_at_unix_ms(),
    )
    .unwrap();
    assert_eq!(retired_restore.state(), SessionState::Created);
    assert_eq!(retired_restore.instrument_version_ref(), VERSION_REF);
}

#[test]
fn persisted_created_identity_rejects_invalid_stored_fields() {
    assert_eq!(
        AssessmentSession::from_persisted_created(
            "12345",
            PARTICIPANT_REF,
            RELEASE_REF,
            VERSION_REF,
            VALID_DIGEST,
            "ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            "12345",
            RELEASE_REF,
            VERSION_REF,
            VALID_DIGEST,
            "ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            PARTICIPANT_REF,
            "12345",
            VERSION_REF,
            VALID_DIGEST,
            "ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            PARTICIPANT_REF,
            RELEASE_REF,
            "12345",
            VALID_DIGEST,
            "ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            PARTICIPANT_REF,
            RELEASE_REF,
            VERSION_REF,
            "sha256:not-a-digest",
            "ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidContentDigest)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            PARTICIPANT_REF,
            RELEASE_REF,
            VERSION_REF,
            VALID_DIGEST,
            " ko-KR",
            20_000,
        ),
        Err(SessionReconstitutionError::InvalidLocale)
    );
    assert_eq!(
        AssessmentSession::from_persisted_created(
            SESSION_REF,
            PARTICIPANT_REF,
            RELEASE_REF,
            VERSION_REF,
            VALID_DIGEST,
            "ko-KR",
            0,
        ),
        Err(SessionReconstitutionError::InvalidTimestamp)
    );
}

#[test]
fn persisted_identity_errors_tell_the_caller_what_to_fix() {
    assert_eq!(
        SessionReconstitutionError::InvalidReference.to_string(),
        "use an opaque non-numeric session, participant, release, or version reference"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidTimestamp.to_string(),
        "use a stored creation time greater than zero"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidContentDigest.to_string(),
        "use a sha256 digest with 64 lowercase hexadecimal digits"
    );
    assert_eq!(
        SessionReconstitutionError::InvalidLocale.to_string(),
        "use an exact whitespace-free BCP 47-style locale tag"
    );
}
