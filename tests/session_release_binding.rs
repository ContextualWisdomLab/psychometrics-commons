//! Session-creation integration contract for immutable published instrument releases.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCreationError, SessionState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
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
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
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
fn published_release_binds_created_session_to_exact_release_and_locale() {
    let release = published_release();
    let session = AssessmentSession::new(
        "ses_02fe09e373504b7986ae78491116edbd",
        PARTICIPANT_REF,
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();

    assert_eq!(
        session.session_ref(),
        "ses_02fe09e373504b7986ae78491116edbd"
    );
    assert_eq!(session.participant_ref(), PARTICIPANT_REF);
    assert_eq!(session.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(
        session.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(session.instrument_release_content_digest(), VALID_DIGEST);
    assert_eq!(session.locale(), "ko-KR");
    assert_eq!(session.created_at_unix_ms(), 20_000);
    assert_eq!(session.state(), SessionState::Created);
}

#[test]
fn later_release_withdrawal_does_not_rewrite_existing_session_provenance() {
    let mut suspended_release = published_release();
    let suspended_session = AssessmentSession::new(
        "ses_cfa319dbad02431a8db35b1ae88f22c8",
        PARTICIPANT_REF,
        &suspended_release,
        "ko-KR",
        20_000,
    )
    .unwrap();
    suspended_release
        .apply_command(
            "publication_suspend_a5b9472f",
            PublicationCommand::Suspend,
            20_100,
        )
        .unwrap();
    assert!(!suspended_release.accepts_new_sessions());
    assert_eq!(
        suspended_session.instrument_release_ref(),
        "release_big_five_ko_v1"
    );
    assert_eq!(
        suspended_session.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(
        suspended_session.instrument_release_content_digest(),
        VALID_DIGEST
    );
    assert_eq!(suspended_session.locale(), "ko-KR");

    let mut retired_release = published_release();
    let retired_session = AssessmentSession::new(
        "ses_0d630c21194e4b93bb7cfb4c87665bd9",
        PARTICIPANT_REF,
        &retired_release,
        "ko-KR",
        21_000,
    )
    .unwrap();
    retired_release
        .apply_command(
            "publication_retire_b6ca5830",
            PublicationCommand::Retire,
            21_100,
        )
        .unwrap();
    assert!(!retired_release.accepts_new_sessions());
    assert_eq!(
        retired_session.instrument_release_ref(),
        "release_big_five_ko_v1"
    );
    assert_eq!(
        retired_session.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(
        retired_session.instrument_release_content_digest(),
        VALID_DIGEST
    );
    assert_eq!(retired_session.locale(), "ko-KR");
}

#[test]
fn session_creation_rejects_nonpublished_release_and_locale_mismatch() {
    let draft = InstrumentRelease::new(manifest(), 10_000).unwrap();
    assert_eq!(
        AssessmentSession::new(
            "ses_6ce2f3b539ce49dd84606000a9fe0cf1",
            PARTICIPANT_REF,
            &draft,
            "ko-KR",
            20_000,
        ),
        Err(SessionCreationError::InstrumentReleaseUnavailable)
    );

    let mut suspended = published_release();
    suspended
        .apply_command(
            "publication_suspend_742f8862",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    assert_eq!(
        AssessmentSession::new(
            "ses_65dbb7b745154f3ea252cc11ce419c3b",
            PARTICIPANT_REF,
            &suspended,
            "ko-KR",
            20_000,
        ),
        Err(SessionCreationError::InstrumentReleaseUnavailable)
    );

    let mut retired = published_release();
    retired
        .apply_command(
            "publication_retire_853f9973",
            PublicationCommand::Retire,
            10_300,
        )
        .unwrap();
    assert_eq!(
        AssessmentSession::new(
            "ses_8e6ecbd3d4e64f019e4526d64d0d6288",
            PARTICIPANT_REF,
            &retired,
            "ko-KR",
            20_000,
        ),
        Err(SessionCreationError::InstrumentReleaseUnavailable)
    );

    let published = published_release();
    assert_eq!(
        AssessmentSession::new(
            "ses_ae8463e84a894610a49e968d6de2e8e9",
            PARTICIPANT_REF,
            &published,
            "en-US",
            20_000,
        ),
        Err(SessionCreationError::LocaleMismatch)
    );
}

#[test]
fn session_creation_rejects_invalid_identity_and_timestamp() {
    let release = published_release();
    assert_eq!(
        AssessmentSession::new("12345", PARTICIPANT_REF, &release, "ko-KR", 20_000),
        Err(SessionCreationError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::new(
            "ses_04d76c1b48df4210bd61027fe292f63d",
            "12345",
            &release,
            "ko-KR",
            20_000,
        ),
        Err(SessionCreationError::InvalidReference)
    );
    assert_eq!(
        AssessmentSession::new(
            "ses_1594e879cb0749a1a373d74d82bbdbcf",
            PARTICIPANT_REF,
            &release,
            "ko-KR",
            0,
        ),
        Err(SessionCreationError::InvalidTimestamp)
    );
}

#[test]
fn session_creation_errors_are_safe_and_specific() {
    assert_eq!(
        SessionCreationError::InvalidReference.to_string(),
        "assessment session references must use their exact opaque non-numeric spelling"
    );
    assert_eq!(
        SessionCreationError::InvalidTimestamp.to_string(),
        "assessment session creation time must be greater than zero"
    );
    assert_eq!(
        SessionCreationError::InstrumentReleaseUnavailable.to_string(),
        "assessment session requires an instrument release currently published for new sessions"
    );
    assert_eq!(
        SessionCreationError::LocaleMismatch.to_string(),
        "assessment session locale must exactly match the published instrument release locale"
    );
}
