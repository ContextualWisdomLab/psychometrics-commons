//! BCP 47 locale contract evidence for deterministic Personality Style identity.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};

fn input(locale: &str) -> StyleAssignmentIdentity<'_> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: "sha256:rule-bundle-a",
        locale,
    }
}

#[test]
fn bcp47_locale_contract_accepts_well_formed_language_tags() {
    for locale in [
        "en",
        "en-US",
        "zh-Hant-TW",
        "sl-rozaj-biske-1994",
        "en-US-u-ca-gregory",
        "x-private",
        "i-klingon",
    ] {
        assert!(
            input(locale).canonical_bytes().is_ok(),
            "well-formed BCP 47 locale must be accepted: {locale}"
        );
    }
}

#[test]
fn bcp47_locale_contract_rejects_malformed_language_tags() {
    for locale in [
        "ko_KR",
        "a-DE",
        "de-419-DE",
        "en--US",
        "ar-a-aaa-b-bbb-a-ccc",
        "sl-rozaj-rozaj",
    ] {
        assert!(
            input(locale).canonical_bytes().is_err(),
            "malformed BCP 47 locale must fail closed: {locale}"
        );
    }
}
