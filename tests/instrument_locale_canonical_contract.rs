//! Contract tests for exact BCP 47 spelling at the instrument-release boundary.

use psychometrics_commons_runtime::instrument::{
    InstrumentReleaseError, InstrumentReleaseManifest, PublicationEvidenceProvenance,
    PublicationEvidenceRecord, PublicationEvidenceStatus,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn manifest_with_locale(locale: &str) -> Result<InstrumentReleaseManifest, InstrumentReleaseError> {
    InstrumentReleaseManifest::new(
        "release_big_five_en_v1",
        "instrument_big_five",
        "instrument_version_big_five_en_v1",
        "construct_big_five",
        &["item_version_001"],
        locale,
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_en_v1",
        None,
        "narrative_version_big_five_v1",
        &[],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        VALID_DIGEST,
    )
}

fn evidence_with_locale(locale: &str) -> Result<PublicationEvidenceRecord, InstrumentReleaseError> {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_en_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_en_v1",
        "instrument_version_big_five_en_v1",
        &["item_version_001"],
        VALID_DIGEST,
        locale,
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_en_v1",
        None,
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_000,
            None,
        )
        .unwrap(),
        &[],
        &[],
        &[],
        PublicationEvidenceStatus::Unknown,
    )
}

#[test]
fn release_locale_must_arrive_in_exact_whitespace_free_spelling() {
    for locale in [" en-US", "en-US ", "\ten-US", "en-US\n", "\u{00a0}en-US"] {
        assert_eq!(
            manifest_with_locale(locale),
            Err(InstrumentReleaseError::InvalidLocale),
            "locale {locale:?} must not be silently normalized"
        );
    }
}

#[test]
fn publication_evidence_locale_must_not_normalize_to_a_release_alias() {
    for locale in [" en-US", "en-US ", "\ten-US", "en-US\n", "\u{00a0}en-US"] {
        assert_eq!(
            evidence_with_locale(locale),
            Err(InstrumentReleaseError::InvalidLocale),
            "evidence locale {locale:?} must not be silently normalized"
        );
    }
}

#[test]
fn exact_bcp47_style_locale_spelling_remains_accepted_unchanged() {
    let manifest = manifest_with_locale("en-US").unwrap();

    assert_eq!(manifest.locale(), "en-US");
    assert!(evidence_with_locale("en-US").is_ok());
}
