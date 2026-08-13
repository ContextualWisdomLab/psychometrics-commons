//! Aggregate-level lifecycle contract for release-bound assessment sessions.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

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
        .apply_command("submit_review", PublicationCommand::SubmitReview, 10_100)
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command("publish", PublicationCommand::Publish, 10_200)
        .unwrap();
    release
}

#[test]
fn aggregate_applies_session_commands_and_preserves_release_provenance() {
    let release = published_release();
    let mut session = AssessmentSession::new(
        "assessment_session_lifecycle",
        "assessment_participant_lifecycle",
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();

    assert_eq!(
        session.apply_command(SessionCommand::Activate).unwrap(),
        SessionState::Active
    );
    assert_eq!(
        session.apply_command(SessionCommand::Pause).unwrap(),
        SessionState::Paused
    );
    assert_eq!(session.state(), SessionState::Paused);
    assert_eq!(session.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(session.locale(), "ko-KR");
}
