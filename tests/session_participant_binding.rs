//! Session creation must inherit participant ownership rather than accept a free-floating ID.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCreationError};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
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
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
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
    .unwrap();

    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command("submit_review_session_owner", PublicationCommand::SubmitReview, 10_100)
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command("publish_session_owner", PublicationCommand::Publish, 10_200)
        .unwrap();
    release
}

#[test]
fn session_inherits_exact_participant_and_tenant_identity() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 15_000).unwrap();
    let session = AssessmentSession::new(
        "session_alpha",
        &participant,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap();

    assert_eq!(session.participant_ref(), "participant_alpha");
    assert_eq!(session.tenant_ref(), "tenant_alpha");
}

#[test]
fn session_creation_cannot_predate_its_participant() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 20_000).unwrap();
    let error = AssessmentSession::new(
        "session_alpha",
        &participant,
        &published_release(),
        "ko-KR",
        19_999,
    )
    .unwrap_err();

    assert_eq!(error, SessionCreationError::ParticipantNotYetCreated);
}
