//! Regression coverage for fail-closed locale control-character validation.

use psychometrics_commons_runtime::narrative::{
    ScoreIdentity, StyleAssignmentIdentity, StyleAssignmentIdentityError,
};

const RULE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn internal_locale_control_character_fails_closed() {
    let control = char::from_u32(1).expect("U+0001 is a valid control character");
    let locale = format!("en{control}US");
    let identity = StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: &locale,
    };

    assert_eq!(
        identity.canonical_bytes(),
        Err(StyleAssignmentIdentityError::NonCanonicalToken)
    );
}
