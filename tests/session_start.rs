//! Session start must use a currently published release, never reconstitution.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::{
    created_session_for_start, created_session_for_start_from_published_snapshot,
    AssessmentSessionStartError,
};
use psychometrics_commons_runtime::postgres_instrument_release::PublishedInstrumentReleaseSnapshot;
use psychometrics_commons_runtime::session::{AssessmentSession, SessionState};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SESSION_REF: &str = "ses_start_published_alpha";

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
fn start_uses_published_release_and_never_reconstitution() {
    let release = published_release();
    let started =
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &release, "ko-KR", 20_000).unwrap();
    let via_new =
        AssessmentSession::new(SESSION_REF, PARTICIPANT_REF, &release, "ko-KR", 20_000).unwrap();

    assert_eq!(started, via_new);
    assert_eq!(started.state(), SessionState::Created);
    assert_eq!(started.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(started.locale(), "ko-KR");
}

#[test]
fn start_rejects_unpublished_release_and_locale_mismatch() {
    let draft = InstrumentRelease::new(manifest(), 10_000).unwrap();
    assert!(matches!(
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &draft, "ko-KR", 20_000),
        Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
    ));

    let mut suspended = published_release();
    suspended
        .apply_command(
            "publication_suspend_start_742f8862",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    assert!(matches!(
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &suspended, "ko-KR", 20_000),
        Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
    ));

    let mut retired = published_release();
    retired
        .apply_command(
            "publication_suspend_before_retire_start",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    retired
        .apply_command(
            "publication_retire_start_9c2e1a0b",
            PublicationCommand::Retire,
            10_400,
        )
        .unwrap();
    assert!(matches!(
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &retired, "ko-KR", 20_000),
        Err(AssessmentSessionStartError::InstrumentReleaseUnavailable)
    ));

    let published = published_release();
    assert!(matches!(
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &published, "en-US", 20_000),
        Err(AssessmentSessionStartError::LocaleMismatch)
    ));
    assert!(matches!(
        created_session_for_start("12345", PARTICIPANT_REF, &published, "ko-KR", 20_000),
        Err(AssessmentSessionStartError::InvalidReference)
    ));
    assert!(matches!(
        created_session_for_start(SESSION_REF, PARTICIPANT_REF, &published, "ko-KR", 0),
        Err(AssessmentSessionStartError::InvalidTimestamp)
    ));
    assert!(matches!(
        AssessmentSession::from_currently_published_manifest(
            SESSION_REF,
            PARTICIPANT_REF,
            published.manifest(),
            "ko-KR",
            0,
        ),
        Err(psychometrics_commons_runtime::session::SessionCreationError::InvalidTimestamp)
    ));
}

#[test]
fn start_errors_tell_the_caller_what_to_do_next() {
    assert_eq!(
        AssessmentSessionStartError::InvalidReference.to_string(),
        "use opaque non-numeric session and participant references to start a session"
    );
    assert_eq!(
        AssessmentSessionStartError::InvalidTimestamp.to_string(),
        "use a server creation time greater than zero to start a session"
    );
    assert_eq!(
        AssessmentSessionStartError::InstrumentReleaseUnavailable.to_string(),
        "publish the exact instrument release before starting a new session"
    );
    assert_eq!(
        AssessmentSessionStartError::LocaleMismatch.to_string(),
        "start the session with the exact published release locale"
    );
    assert_eq!(
        AssessmentSessionStartError::InvalidStoredRelease.to_string(),
        "repair the stored instrument release before starting a new session"
    );
    let persistence = AssessmentSessionStartError::Persistence(
        psychometrics_commons_runtime::postgres_assessment_session::AssessmentSessionPersistenceError::ConflictingReplay,
    );
    assert_eq!(
        persistence.to_string(),
        "session start could not persist the created session; retry the exact start or repair the store"
    );
    assert!(std::error::Error::source(&persistence).is_some());
    assert!(std::error::Error::source(&AssessmentSessionStartError::LocaleMismatch).is_none());
}

#[test]
fn start_from_published_snapshot_matches_new_and_rejects_locale_mismatch() {
    let release = published_release();
    let snapshot = PublishedInstrumentReleaseSnapshot::from_published_manifest(
        release.manifest().clone(),
        release.created_at_unix_ms(),
    )
    .unwrap();
    let started = created_session_for_start_from_published_snapshot(
        SESSION_REF,
        PARTICIPANT_REF,
        &snapshot,
        "ko-KR",
        20_000,
    )
    .unwrap();
    let via_new =
        AssessmentSession::new(SESSION_REF, PARTICIPANT_REF, &release, "ko-KR", 20_000).unwrap();

    assert_eq!(started, via_new);
    assert_eq!(started.state(), SessionState::Created);
    assert!(matches!(
        created_session_for_start_from_published_snapshot(
            SESSION_REF,
            PARTICIPANT_REF,
            &snapshot,
            "en-US",
            20_000,
        ),
        Err(AssessmentSessionStartError::LocaleMismatch)
    ));
    assert!(matches!(
        created_session_for_start_from_published_snapshot(
            "12345",
            PARTICIPANT_REF,
            &snapshot,
            "ko-KR",
            20_000,
        ),
        Err(AssessmentSessionStartError::InvalidReference)
    ));
    assert!(matches!(
        PublishedInstrumentReleaseSnapshot::from_published_manifest(release.manifest().clone(), 0),
        Err(
            psychometrics_commons_runtime::postgres_instrument_release::InstrumentReleaseQueryError::InvalidStoredValue
        )
    ));
}
