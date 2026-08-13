//! SHA-256 key evidence for deterministic Personality Style assignment.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};

const RULE_DIGEST_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input(score_identity: ScoreIdentity<'_>) -> StyleAssignmentIdentity<'_> {
    StyleAssignmentIdentity {
        score_identity,
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST_A,
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
            0x25, 0x6d, 0x93, 0xed, 0xaa, 0x43, 0xbf, 0x95, 0xf3, 0xae, 0xba, 0xe0, 0x0c, 0xcd,
            0xb6, 0x57, 0xeb, 0xcf, 0xb1, 0x89, 0x92, 0xa6, 0x84, 0xbf, 0x8c, 0x0a, 0x20, 0x7d,
            0x41, 0x28, 0xad, 0xf5,
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
