//! Contract tests for exact BCP 47 spelling at the instrument-release boundary.

use psychometrics_commons_runtime::instrument::{InstrumentReleaseError, InstrumentReleaseManifest};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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
fn exact_bcp47_style_locale_spelling_remains_accepted_unchanged() {
    let manifest = manifest_with_locale("en-US").unwrap();

    assert_eq!(manifest.locale(), "en-US");
}
