//! Deterministic localized fallback evidence for Personality Style presentation.

use psychometrics_commons_runtime::narrative::{
    ApprovedNarrativeFallback, NarrativeFallbackError, ScoreIdentity, StyleAssignmentIdentity,
};

const RULE_DIGEST_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RULE_DIGEST_B: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EN_TEXT: &str = "You tend to explore ideas before settling on one interpretation.";
const EN_TEXT_DIGEST: &str =
    "sha256:e7940e6ede35f3b3df2c37e040db18ed4752fcd4f00bed91a8a26767b8a61d71";

fn identity(locale: &str, rule_digest: &str) -> StyleAssignmentIdentity<'_> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_fallback_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: rule_digest,
        locale,
    }
}

fn fallback<'a>(
    assignment: &StyleAssignmentIdentity<'_>,
    locale: &'a str,
    rule_digest: &'a str,
    content_digest: &'a str,
    text: &'a str,
) -> ApprovedNarrativeFallback<'a> {
    ApprovedNarrativeFallback {
        style_assignment_key: assignment.assignment_key().unwrap(),
        narrative_version_ref: "narrative_version_en_v1",
        interpretation_rule_bundle_digest: rule_digest,
        locale,
        content_digest,
        text,
    }
}

#[test]
fn exact_approved_bundle_finalizes_without_optional_ai() {
    let assignment = identity("en-US", RULE_DIGEST_A);
    let bundle = fallback(
        &assignment,
        "en-US",
        RULE_DIGEST_A,
        EN_TEXT_DIGEST,
        EN_TEXT,
    );

    let finalized = bundle.finalize_for(&assignment).unwrap();

    assert_eq!(finalized.style_assignment_key(), bundle.style_assignment_key);
    assert_eq!(finalized.narrative_version_ref(), "narrative_version_en_v1");
    assert_eq!(finalized.locale(), "en-US");
    assert_eq!(finalized.content_digest(), EN_TEXT_DIGEST);
    assert_eq!(finalized.text(), EN_TEXT);
}

#[test]
fn altered_fallback_bytes_fail_closed_on_content_digest() {
    let assignment = identity("en-US", RULE_DIGEST_A);
    let bundle = fallback(
        &assignment,
        "en-US",
        RULE_DIGEST_A,
        EN_TEXT_DIGEST,
        "You tend to explore ideas before settling on one interpretation!",
    );

    assert_eq!(
        bundle.finalize_for(&assignment),
        Err(NarrativeFallbackError::ContentDigestMismatch)
    );
}

#[test]
fn fallback_must_match_exact_assignment_locale() {
    let assignment = identity("en-US", RULE_DIGEST_A);
    let bundle = fallback(
        &assignment,
        "ko-KR",
        RULE_DIGEST_A,
        EN_TEXT_DIGEST,
        EN_TEXT,
    );

    assert_eq!(
        bundle.finalize_for(&assignment),
        Err(NarrativeFallbackError::LocaleMismatch)
    );
}

#[test]
fn fallback_must_match_approved_interpretation_rules() {
    let assignment = identity("en-US", RULE_DIGEST_A);
    let bundle = fallback(
        &assignment,
        "en-US",
        RULE_DIGEST_B,
        EN_TEXT_DIGEST,
        EN_TEXT,
    );

    assert_eq!(
        bundle.finalize_for(&assignment),
        Err(NarrativeFallbackError::InterpretationRuleMismatch)
    );
}

#[test]
fn fallback_cannot_be_rebound_to_another_style_assignment() {
    let original = identity("en-US", RULE_DIGEST_A);
    let different = StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_fallback_beta"),
        ..identity("en-US", RULE_DIGEST_A)
    };
    let bundle = fallback(
        &original,
        "en-US",
        RULE_DIGEST_A,
        EN_TEXT_DIGEST,
        EN_TEXT,
    );

    assert_eq!(
        bundle.finalize_for(&different),
        Err(NarrativeFallbackError::AssignmentKeyMismatch)
    );
}

#[test]
fn fallback_rejects_invalid_identity_bundle_fields_and_empty_text() {
    let assignment = identity("en-US", RULE_DIGEST_A);

    let bad_reference = ApprovedNarrativeFallback {
        narrative_version_ref: "42",
        ..fallback(
            &assignment,
            "en-US",
            RULE_DIGEST_A,
            EN_TEXT_DIGEST,
            EN_TEXT,
        )
    };
    assert_eq!(
        bad_reference.finalize_for(&assignment),
        Err(NarrativeFallbackError::InvalidReference)
    );

    let bad_locale = fallback(
        &assignment,
        "en_US",
        RULE_DIGEST_A,
        EN_TEXT_DIGEST,
        EN_TEXT,
    );
    assert_eq!(
        bad_locale.finalize_for(&assignment),
        Err(NarrativeFallbackError::NonCanonicalToken)
    );

    let bad_rule_digest = fallback(
        &assignment,
        "en-US",
        "sha256:ABC",
        EN_TEXT_DIGEST,
        EN_TEXT,
    );
    assert_eq!(
        bad_rule_digest.finalize_for(&assignment),
        Err(NarrativeFallbackError::NonCanonicalToken)
    );

    let bad_content_digest = fallback(
        &assignment,
        "en-US",
        RULE_DIGEST_A,
        "sha256:ABC",
        EN_TEXT,
    );
    assert_eq!(
        bad_content_digest.finalize_for(&assignment),
        Err(NarrativeFallbackError::NonCanonicalToken)
    );

    let empty_text = fallback(
        &assignment,
        "en-US",
        RULE_DIGEST_A,
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "",
    );
    assert_eq!(
        empty_text.finalize_for(&assignment),
        Err(NarrativeFallbackError::EmptyText)
    );
}

#[test]
fn fallback_propagates_invalid_style_assignment_identity() {
    let invalid_assignment = identity("en_US", RULE_DIGEST_A);
    let bundle = ApprovedNarrativeFallback {
        style_assignment_key: identity("en-US", RULE_DIGEST_A).assignment_key().unwrap(),
        narrative_version_ref: "narrative_version_en_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST_A,
        locale: "en-US",
        content_digest: EN_TEXT_DIGEST,
        text: EN_TEXT,
    };

    assert_eq!(
        bundle.finalize_for(&invalid_assignment),
        Err(NarrativeFallbackError::InvalidAssignmentIdentity)
    );
}

#[test]
fn fallback_error_messages_are_operator_safe() {
    assert_eq!(
        NarrativeFallbackError::AssignmentKeyMismatch.to_string(),
        "narrative fallback does not belong to the style assignment"
    );
    assert_eq!(
        NarrativeFallbackError::InterpretationRuleMismatch.to_string(),
        "narrative fallback interpretation rules do not match the style assignment"
    );
    assert_eq!(
        NarrativeFallbackError::LocaleMismatch.to_string(),
        "narrative fallback locale does not match the style assignment"
    );
    assert_eq!(
        NarrativeFallbackError::ContentDigestMismatch.to_string(),
        "narrative fallback content digest does not match its text"
    );
    assert_eq!(
        NarrativeFallbackError::InvalidReference.to_string(),
        "narrative fallback references must be opaque non-numeric values"
    );
    assert_eq!(
        NarrativeFallbackError::NonCanonicalToken.to_string(),
        "narrative fallback digests and locale must be canonical tokens"
    );
    assert_eq!(
        NarrativeFallbackError::EmptyText.to_string(),
        "narrative fallback text must not be empty"
    );
    assert_eq!(
        NarrativeFallbackError::InvalidAssignmentIdentity.to_string(),
        "narrative fallback style-assignment identity is invalid"
    );
}
