//! SHA-256 key evidence for deterministic Personality Style assignment.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};

fn input(score_identity: ScoreIdentity<'_>) -> StyleAssignmentIdentity<'_> {
    StyleAssignmentIdentity {
        score_identity,
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: "sha256:rule-bundle-a",
        locale: "en-US",
    }
}

#[test]
fn style_assignment_key_matches_the_adr_sha256_test_vector() {
    let key = input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        .assignment_key()
        .unwrap();

    assert_eq!(
        key.as_bytes(),
        &[
            0xd5, 0xaf, 0x67, 0x5b, 0x9f, 0x29, 0x47, 0x20, 0xb4, 0xbe, 0x09, 0x37, 0x8a, 0xa9,
            0x7c, 0x5e, 0x00, 0xe3, 0x6e, 0x73, 0x01, 0x75, 0x78, 0xf4, 0x4b, 0x00, 0x3b, 0x4e,
            0x7d, 0xff, 0xe4, 0x0a,
        ]
    );
}

#[test]
fn normalized_equivalent_identity_has_the_same_key() {
    let canonical = input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        .assignment_key()
        .unwrap();
    let normalized = input(ScoreIdentity::ScoreProfileRef(" score_profile_alpha "))
        .assignment_key()
        .unwrap();

    assert_eq!(normalized, canonical);
}

#[test]
fn behavior_change_has_a_different_key() {
    let baseline = input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        .assignment_key()
        .unwrap();
    let changed = StyleAssignmentIdentity {
        locale: "ko-KR",
        ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
    }
    .assignment_key()
    .unwrap();

    assert_ne!(changed, baseline);
}

#[test]
fn key_construction_preserves_fail_closed_identity_validation() {
    let invalid = StyleAssignmentIdentity {
        locale: "ko_KR",
        ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
    };

    assert!(invalid.assignment_key().is_err());
}
