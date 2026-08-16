//! Canonical identity evidence for deterministic Personality Style assignment.

use psychometrics_commons_runtime::narrative::{
    ScoreIdentity, StyleAssignmentIdentity, StyleAssignmentIdentityError,
};

const RULE_DIGEST_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RULE_DIGEST_B: &str =
    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const SCORE_DIGEST_A: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
fn canonical_serialization_is_stable_and_self_delimiting() {
    let canonical = input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        .canonical_bytes()
        .unwrap();

    let expected = [
        b"psychometrics-commons/style-assignment-identity/v1\0".as_slice(),
        b"score_identity_kind\0\0\0\0\0\0\0\x11score_profile_ref".as_slice(),
        b"score_identity\0\0\0\0\0\0\0\x13score_profile_alpha".as_slice(),
        b"instrument_version_ref\0\0\0\0\0\0\0\x26instrument_version_ipip_big_five_en_v1"
            .as_slice(),
        b"scoring_version_ref\0\0\0\0\0\0\0\x1bscoring_version_big_five_v1".as_slice(),
        b"norm_version_ref_present\0\0\0\0\0\0\0\x011".as_slice(),
        b"norm_version_ref\0\0\0\0\0\0\0\x19norm_version_reference_v1".as_slice(),
        b"style_mapping_version_ref\0\0\0\0\0\0\0\x18style_mapping_version_v1".as_slice(),
        b"interpretation_rule_bundle_digest\0\0\0\0\0\0\0\x47sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .as_slice(),
        b"locale\0\0\0\0\0\0\0\x05en-US".as_slice(),
    ]
    .concat();

    assert_eq!(canonical, expected);
}

#[test]
fn every_behavior_affecting_field_changes_canonical_identity() {
    let baseline = input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        .canonical_bytes()
        .unwrap();

    let variants = [
        input(ScoreIdentity::ScoreProfileRef("score_profile_beta")),
        input(ScoreIdentity::CanonicalScorePayloadDigest(SCORE_DIGEST_A)),
        StyleAssignmentIdentity {
            instrument_version_ref: "instrument_version_ipip_big_five_en_v2",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            scoring_version_ref: "scoring_version_big_five_v2",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            norm_version_ref: None,
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            norm_version_ref: Some("norm_version_reference_v2"),
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            style_mapping_version_ref: "style_mapping_version_v2",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            interpretation_rule_bundle_digest: RULE_DIGEST_B,
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            locale: "ko-KR",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
    ];

    for variant in variants {
        assert_ne!(variant.canonical_bytes().unwrap(), baseline);
    }
}

#[test]
fn references_and_exact_tokens_reject_noncanonical_spelling() {
    for invalid_reference in [
        " score_profile_alpha ",
        "score_profile_alpha\t",
        "score\nprofile_alpha",
        "score\u{200b}profile_alpha",
        "score\u{202e}profile_alpha",
    ] {
        assert_eq!(
            input(ScoreIdentity::ScoreProfileRef(invalid_reference)).canonical_bytes(),
            Err(StyleAssignmentIdentityError::InvalidReference)
        );
    }

    for invalid in [
        StyleAssignmentIdentity {
            interpretation_rule_bundle_digest:
                " sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            locale: " en-US ",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        input(ScoreIdentity::CanonicalScorePayloadDigest(
            " sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ",
        )),
    ] {
        assert_eq!(
            invalid.canonical_bytes(),
            Err(StyleAssignmentIdentityError::NonCanonicalToken)
        );
    }
}

#[test]
fn missing_or_numeric_like_references_fail_closed() {
    for invalid in [
        input(ScoreIdentity::ScoreProfileRef("12345")),
        StyleAssignmentIdentity {
            instrument_version_ref: " ",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            scoring_version_ref: "42",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            norm_version_ref: Some("3.14"),
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            style_mapping_version_ref: "-7",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
    ] {
        assert_eq!(
            invalid.canonical_bytes(),
            Err(StyleAssignmentIdentityError::InvalidReference)
        );
    }
}

#[test]
fn noncanonical_exact_tokens_fail_closed() {
    for invalid in [
        input(ScoreIdentity::CanonicalScorePayloadDigest("")),
        input(ScoreIdentity::CanonicalScorePayloadDigest(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\0",
        )),
        StyleAssignmentIdentity {
            interpretation_rule_bundle_digest: "   ",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            locale: "",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
        StyleAssignmentIdentity {
            locale: "en US",
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        },
    ] {
        assert_eq!(
            invalid.canonical_bytes(),
            Err(StyleAssignmentIdentityError::NonCanonicalToken)
        );
    }
}

#[test]
fn locale_requires_bcp47_subtag_structure() {
    for valid in [
        "en",
        "abcd",
        "abcdefg",
        "ko-KR",
        "zh-Hant-TW",
        "es-419",
        "zh-cmn-Hans-CN",
        "zh-cmn-yue-hak-Hans-CN",
        "sl-rozaj-biske",
        "de-CH-1901",
        "en-US-u-ca-gregory",
        "en-US-a-abc-b-1234-x-private",
        "en-u-ca-x-private",
        "zh-Hant-TW-x-private",
        "i-klingon",
        "EN-gb-OED",
        "x-private",
        "x-a-b-c",
    ] {
        let identity = StyleAssignmentIdentity {
            locale: valid,
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        };
        assert!(
            identity.canonical_bytes().is_ok(),
            "expected valid locale: {valid}"
        );
    }

    for invalid in [
        "ko_KR",
        "-en-US",
        "en-US-",
        "en--US",
        "e-US",
        "123-US",
        "1abc",
        "englishish-US",
        "en-abcdefghi",
        "en-US_foo",
        "en-a",
        "en-u-a",
        "en-u-abcdefghi",
        "en-US-foo",
        "en-u-ca-gregory-u-nu-latn",
        "en-x",
        "x",
        "x-abcdefghi",
        "en-x-abcdefghi",
        "en-US-Latn",
        "en-419-US",
        "en-12",
        "en-_",
        "en-1_23",
        "sl-rozaj-rozaj",
        "zh-cmn-yue-hak-jkl-Hans-CN",
    ] {
        let identity = StyleAssignmentIdentity {
            locale: invalid,
            ..input(ScoreIdentity::ScoreProfileRef("score_profile_alpha"))
        };
        assert_eq!(
            identity.canonical_bytes(),
            Err(StyleAssignmentIdentityError::NonCanonicalToken),
            "expected malformed BCP 47 locale to fail closed: {invalid}"
        );
    }
}

#[test]
fn identity_errors_expose_stable_operator_messages() {
    assert_eq!(
        StyleAssignmentIdentityError::InvalidReference.to_string(),
        "style-assignment references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters"
    );
    assert_eq!(
        StyleAssignmentIdentityError::NonCanonicalToken.to_string(),
        "style-assignment digests and locale must be nonblank canonical tokens"
    );
}
