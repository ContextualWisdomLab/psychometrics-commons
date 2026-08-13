//! Fail-closed digest validation for deterministic Personality Style provenance.

use psychometrics_commons_runtime::narrative::{
    ScoreIdentity, StyleAssignmentIdentity, StyleAssignmentIdentityError,
};

const VALID_SCORE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const VALID_RULE_DIGEST: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn identity<'a>(score_digest: &'a str, rule_digest: &'a str) -> StyleAssignmentIdentity<'a> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::CanonicalScorePayloadDigest(score_digest),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: rule_digest,
        locale: "en-US",
    }
}

#[test]
fn canonical_sha256_digests_are_accepted() {
    assert!(identity(VALID_SCORE_DIGEST, VALID_RULE_DIGEST)
        .canonical_bytes()
        .is_ok());
}

#[test]
fn malformed_provenance_digests_fail_closed() {
    for invalid in [
        "sha256:score-a",
        "sha512:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef",
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(
            identity(invalid, VALID_RULE_DIGEST).canonical_bytes(),
            Err(StyleAssignmentIdentityError::NonCanonicalToken)
        );
        assert_eq!(
            identity(VALID_SCORE_DIGEST, invalid).canonical_bytes(),
            Err(StyleAssignmentIdentityError::NonCanonicalToken)
        );
    }
}
