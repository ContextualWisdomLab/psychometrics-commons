//! Fail-closed edge evidence for deterministic narrative fallback rendering.

use psychometrics_commons_runtime::deterministic_narrative::{
    ApprovedStyleSelection, DeterministicNarrativeBundle, NarrativeFallbackError, NarrativeUnit,
};
use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};

const RULE_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ALT_RULE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn identity() -> StyleAssignmentIdentity<'static> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: None,
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
    }
}

fn unit() -> NarrativeUnit<'static> {
    NarrativeUnit {
        interpretation_unit_ref: "unit_openness",
        heading: "How you explore",
        body: "Approved deterministic interpretation text.",
    }
}

fn selection() -> ApprovedStyleSelection<'static> {
    ApprovedStyleSelection {
        assignment_key: identity().assignment_key().unwrap(),
        primary_style_ref: "style_exploratory",
        adjacent_style_refs: &[],
        interpretation_unit_refs: &["unit_openness"],
    }
}

fn bundle<'a>(units: &'a [NarrativeUnit<'a>]) -> DeterministicNarrativeBundle<'a> {
    DeterministicNarrativeBundle {
        narrative_version_ref: "narrative_version_en_v1",
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
        units,
        limitations: &["Scores remain continuous scientific evidence."],
    }
}

#[test]
fn invalid_assignment_identity_is_not_downgraded_to_a_provenance_mismatch() {
    let units = [unit()];
    let invalid_identity = StyleAssignmentIdentity {
        locale: "en_US",
        ..identity()
    };

    assert_eq!(
        bundle(&units).render(&invalid_identity, &selection()),
        Err(NarrativeFallbackError::InvalidIdentity)
    );
}

#[test]
fn exact_rule_digest_mismatch_fails_closed_even_when_both_digests_are_valid() {
    let units = [unit()];
    let other_bundle = DeterministicNarrativeBundle {
        interpretation_rule_bundle_digest: ALT_RULE_DIGEST,
        ..bundle(&units)
    };

    assert_eq!(
        other_bundle.render(&identity(), &selection()),
        Err(NarrativeFallbackError::IdentityMismatch)
    );
}

#[test]
fn malformed_digest_prefix_and_noncanonical_hex_are_rejected() {
    let units = [unit()];
    for digest in [
        "md5:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        let invalid = DeterministicNarrativeBundle {
            interpretation_rule_bundle_digest: digest,
            ..bundle(&units)
        };
        assert_eq!(
            invalid.render(&identity(), &selection()),
            Err(NarrativeFallbackError::InvalidDigest)
        );
    }
}

#[test]
fn every_bundle_and_selection_reference_uses_the_opaque_reference_contract() {
    let units = [unit()];
    let invalid_narrative_version = DeterministicNarrativeBundle {
        narrative_version_ref: "12345",
        ..bundle(&units)
    };
    assert_eq!(
        invalid_narrative_version.render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidReference)
    );

    let invalid_adjacent = ApprovedStyleSelection {
        adjacent_style_refs: &["12345"],
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &invalid_adjacent),
        Err(NarrativeFallbackError::InvalidReference)
    );

    let invalid_unit_selection = ApprovedStyleSelection {
        interpretation_unit_refs: &["12345"],
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &invalid_unit_selection),
        Err(NarrativeFallbackError::InvalidReference)
    );

    let invalid_units = [NarrativeUnit {
        interpretation_unit_ref: "12345",
        ..unit()
    }];
    assert_eq!(
        bundle(&invalid_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidReference)
    );
}

#[test]
fn duplicate_adjacent_styles_fail_closed_even_when_primary_is_distinct() {
    let units = [unit()];
    let duplicate = ApprovedStyleSelection {
        adjacent_style_refs: &["style_deliberative", "style_deliberative"],
        ..selection()
    };

    assert_eq!(
        bundle(&units).render(&identity(), &duplicate),
        Err(NarrativeFallbackError::DuplicateReference)
    );
}

#[test]
fn display_text_rejects_padding_controls_and_blank_limitations() {
    let padded_units = [NarrativeUnit {
        heading: " How you explore",
        ..unit()
    }];
    assert_eq!(
        bundle(&padded_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidText)
    );

    let controlled_units = [NarrativeUnit {
        body: "Approved\ntext",
        ..unit()
    }];
    assert_eq!(
        bundle(&controlled_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidText)
    );

    let units = [unit()];
    let blank_limitation = DeterministicNarrativeBundle {
        limitations: &[""],
        ..bundle(&units)
    };
    assert_eq!(
        blank_limitation.render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidText)
    );
}

#[test]
fn deterministic_fallback_requires_at_least_one_participant_limitation() {
    let units = [unit()];
    let missing_limitations = DeterministicNarrativeBundle {
        limitations: &[],
        ..bundle(&units)
    };

    assert_eq!(
        missing_limitations.render(&identity(), &selection()),
        Err(NarrativeFallbackError::MissingLimitations)
    );
}

#[test]
fn bundle_locale_must_be_canonical_display_text_before_identity_comparison() {
    let units = [unit()];
    let padded_locale = DeterministicNarrativeBundle {
        locale: " en-US",
        ..bundle(&units)
    };

    assert_eq!(
        padded_locale.render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidText)
    );
}

#[test]
fn every_fallback_error_has_stable_beginner_readable_display_text() {
    let cases = [
        (
            NarrativeFallbackError::InvalidReference,
            "narrative references must be opaque non-numeric values",
        ),
        (
            NarrativeFallbackError::InvalidText,
            "narrative text must be nonblank canonical display text",
        ),
        (
            NarrativeFallbackError::InvalidDigest,
            "narrative rule digest must be canonical lowercase SHA-256",
        ),
        (
            NarrativeFallbackError::InvalidIdentity,
            "style-assignment identity is invalid",
        ),
        (
            NarrativeFallbackError::IdentityMismatch,
            "narrative provenance does not match style assignment",
        ),
        (
            NarrativeFallbackError::DuplicateReference,
            "narrative style or interpretation reference is duplicated",
        ),
        (
            NarrativeFallbackError::EmptySelection,
            "narrative selection must contain interpretation units",
        ),
        (
            NarrativeFallbackError::MissingLimitations,
            "deterministic narrative bundle must include participant-facing limitations",
        ),
        (
            NarrativeFallbackError::MissingInterpretationUnit,
            "selected interpretation unit is absent from the approved bundle",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
