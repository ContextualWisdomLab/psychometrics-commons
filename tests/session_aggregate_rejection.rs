//! Rejected session commands leave the assessment session in its prior state.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionState, TransitionErrorKind,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn release() -> InstrumentRelease {
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
            "publication_review_8d548615",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_9e659726",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

#[test]
fn rejected_command_preserves_aggregate_state_and_release_provenance() {
    let release = release();
    let mut session = AssessmentSession::new(
        "ses_a780e89390dc46af81ac1045cbf4f945",
        "ptc_d2f8ecfa531147b5817a87051b6c34cf",
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();

    let error = session
        .apply_command(
            "cmd_e6deabff4593406cbb313660cd40bd42",
            1,
            SessionCommand::Release,
        )
        .unwrap_err();
    assert_eq!(error.kind(), TransitionErrorKind::InvalidTransition);
    assert_eq!(error.state(), SessionState::Created);
    assert_eq!(error.command(), SessionCommand::Release);
    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(session.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(session.instrument_release_content_digest(), RELEASE_DIGEST);
    assert_eq!(session.locale(), "ko-KR");
}
