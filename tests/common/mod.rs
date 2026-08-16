//! Shared integration-test fixtures for authoritative assessment-session provenance.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::AssessmentSession;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_release(instrument_version_ref: &str) -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_result_fixture_v1",
        "instrument_result_fixture",
        instrument_version_ref,
        "construct_result_fixture",
        &["item_version_ref"],
        "en-US",
        "assessment_spec_result_fixture",
        "scoring_version_result_fixture",
        "calibration_result_fixture",
        Some("norm_version_result_fixture"),
        "narrative_version_result_fixture",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_result_fixture_v1",
        "evidence_policy_self_reflection_v1",
        "release_result_fixture_v1",
        instrument_version_ref,
        &["item_version_ref"],
        RELEASE_DIGEST,
        "en-US",
        "intended_use_self_reflection_v1",
        "assessment_spec_result_fixture",
        "scoring_version_result_fixture",
        "calibration_result_fixture",
        Some("norm_version_result_fixture"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_result_fixture_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_result_fixture_v1"],
        &["recovery_result_fixture_v1"],
        &["approval_result_fixture_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();

    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_result_fixture",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_result_fixture",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

pub fn assessment_session(
    session_ref: &str,
    participant_ref: &str,
    instrument_version_ref: &str,
) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        participant_ref,
        &published_release(instrument_version_ref),
        "en-US",
        20_000,
    )
    .unwrap()
}
