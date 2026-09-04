//! Exact-spelling contracts for immutable instrument publication identity.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn manifest_with(
    release_ref: &str,
    items: &[&str],
    consent_refs: &[&str],
) -> Result<InstrumentReleaseManifest, InstrumentReleaseError> {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        items,
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        consent_refs,
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        VALID_DIGEST,
    )
}

fn exact_manifest() -> InstrumentReleaseManifest {
    manifest_with(
        "release_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        &["consent_service_v1"],
    )
    .unwrap()
}

fn provenance(
    population_ref: &str,
) -> Result<PublicationEvidenceProvenance, InstrumentReleaseError> {
    PublicationEvidenceProvenance::new(
        EVIDENCE_DIGEST,
        population_ref,
        "administration_web_self_report_v1",
        "measurement_model_big_five_v1",
        10_050,
        None,
    )
}

fn evidence_with_rights(
    publication_evidence_ref: &str,
    rights_refs: &[&str],
) -> Result<PublicationEvidenceRecord, InstrumentReleaseError> {
    PublicationEvidenceRecord::new(
        publication_evidence_ref,
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
        provenance("population_general_adult_v1").unwrap(),
        rights_refs,
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
}

#[test]
fn manifest_rejects_whitespace_aliases_in_scalar_and_collection_references() {
    for result in [
        manifest_with(
            " release_big_five_ko_v1",
            &["item_version_001"],
            &["consent_service_v1"],
        ),
        manifest_with(
            "release_big_five_ko_v1",
            &["item_version_001 "],
            &["consent_service_v1"],
        ),
        manifest_with(
            "release_big_five_ko_v1",
            &["item_version_001"],
            &[" consent_service_v1"],
        ),
    ] {
        assert_eq!(result, Err(InstrumentReleaseError::InvalidReference));
    }
}

#[test]
fn manifest_rejects_reference_that_normalization_cannot_admit() {
    assert_eq!(
        manifest_with("123", &["item_version_001"], &["consent_service_v1"],),
        Err(InstrumentReleaseError::InvalidReference)
    );
}

#[test]
fn publication_evidence_rejects_whitespace_aliases() {
    assert_eq!(
        provenance(" population_general_adult_v1"),
        Err(InstrumentReleaseError::InvalidReference)
    );
    for publication_evidence_ref in [
        " publication_evidence_big_five_ko_v1",
        "publication_evidence_big_five_ko_v1 ",
    ] {
        assert_eq!(
            evidence_with_rights(publication_evidence_ref, &["rights_ipip_big_five_v1"]),
            Err(InstrumentReleaseError::InvalidReference)
        );
    }
    assert_eq!(
        evidence_with_rights(
            "publication_evidence_big_five_ko_v1",
            &["rights_ipip_big_five_v1 "],
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
}

#[test]
fn publication_event_reference_rejects_whitespace_alias() {
    let mut release = InstrumentRelease::new(exact_manifest(), 10_000).unwrap();
    assert_eq!(
        release.apply_command(
            " submit_review_event",
            PublicationCommand::SubmitReview,
            10_100,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
}
