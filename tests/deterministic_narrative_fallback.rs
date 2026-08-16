//! Deterministic fallback evidence for ADR-0018 narrative rendering.

use psychometrics_commons_runtime::deterministic_narrative::{
    ApprovedStyleSelection, DeterministicNarrativeBundle, NarrativeFallbackError, NarrativeUnit,
};
use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};

const RULE_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn identity() -> StyleAssignmentIdentity<'static> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
    }
}

fn selection() -> ApprovedStyleSelection<'static> {
    ApprovedStyleSelection {
        assignment_key: identity().assignment_key().unwrap(),
        primary_style_ref: "style_exploratory",
        adjacent_style_refs: &["style_deliberative"],
        interpretation_unit_refs: &["unit_openness", "unit_conscientiousness"],
    }
}

fn units() -> Vec<NarrativeUnit<'static>> {
    vec![
        NarrativeUnit {
            interpretation_unit_ref: "unit_openness",
            heading: "How you explore",
            body: "Your approved score interpretation emphasizes curiosity without replacing the underlying continuous score.",
        },
        NarrativeUnit {
            interpretation_unit_ref: "unit_conscientiousness",
            heading: "How you organize",
            body: "Your approved score interpretation describes planning tendencies while keeping uncertainty and provenance separate.",
        },
    ]
}

fn bundle<'a>(units: &'a [NarrativeUnit<'a>]) -> DeterministicNarrativeBundle<'a> {
    DeterministicNarrativeBundle {
        narrative_version_ref: "narrative_version_en_v1",
        style_mapping_version_ref: "style_mapping_version_v1",
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
        units,
        limitations: &["This presentation is descriptive and is not an MBTI-equivalence claim."],
    }
}

#[test]
fn approved_selection_renders_deterministically_without_mutating_score_identity() {
    let units = units();
    let rendered = bundle(&units).render(&identity(), &selection()).unwrap();
    let replay = bundle(&units).render(&identity(), &selection()).unwrap();

    assert_eq!(rendered, replay);
    assert_eq!(rendered.assignment_key, selection().assignment_key);
    assert_eq!(rendered.narrative_version_ref, "narrative_version_en_v1");
    assert_eq!(rendered.locale, "en-US");
    assert_eq!(rendered.primary_style_ref, "style_exploratory");
    assert_eq!(rendered.adjacent_style_refs, vec!["style_deliberative"]);
    assert_eq!(rendered.sections.len(), 2);
    assert_eq!(
        rendered.sections[0].interpretation_unit_ref,
        "unit_openness"
    );
    assert_eq!(rendered.sections[1].heading, "How you organize");
    assert_eq!(
        rendered.limitations,
        vec!["This presentation is descriptive and is not an MBTI-equivalence claim."]
    );
}

#[test]
fn mismatched_assignment_key_fails_closed() {
    let units = units();
    let other_identity = StyleAssignmentIdentity {
        locale: "ko-KR",
        ..identity()
    };
    let mismatched = ApprovedStyleSelection {
        assignment_key: other_identity.assignment_key().unwrap(),
        ..selection()
    };

    assert_eq!(
        bundle(&units).render(&identity(), &mismatched),
        Err(NarrativeFallbackError::IdentityMismatch)
    );
}

#[test]
fn bundle_provenance_must_match_the_canonical_assignment_identity() {
    let units = units();
    let mapping_mismatch = DeterministicNarrativeBundle {
        style_mapping_version_ref: "style_mapping_version_v2",
        ..bundle(&units)
    };
    assert_eq!(
        mapping_mismatch.render(&identity(), &selection()),
        Err(NarrativeFallbackError::IdentityMismatch)
    );

    let locale_mismatch = DeterministicNarrativeBundle {
        locale: "ko-KR",
        ..bundle(&units)
    };
    assert_eq!(
        locale_mismatch.render(&identity(), &selection()),
        Err(NarrativeFallbackError::IdentityMismatch)
    );
}

#[test]
fn invalid_bundle_digest_is_rejected_before_rendering() {
    let units = units();
    let invalid = DeterministicNarrativeBundle {
        interpretation_rule_bundle_digest: "sha256:ABCDEF",
        ..bundle(&units)
    };

    assert_eq!(
        invalid.render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidDigest)
    );
}

#[test]
fn duplicate_style_or_interpretation_references_fail_closed() {
    let units = units();
    let duplicate_style = ApprovedStyleSelection {
        adjacent_style_refs: &["style_exploratory"],
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &duplicate_style),
        Err(NarrativeFallbackError::DuplicateReference)
    );

    let duplicate_units = ApprovedStyleSelection {
        interpretation_unit_refs: &["unit_openness", "unit_openness"],
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &duplicate_units),
        Err(NarrativeFallbackError::DuplicateReference)
    );
}

#[test]
fn bundle_rejects_duplicate_or_missing_interpretation_units() {
    let duplicate_bundle_units = vec![
        NarrativeUnit {
            interpretation_unit_ref: "unit_openness",
            heading: "A",
            body: "A body",
        },
        NarrativeUnit {
            interpretation_unit_ref: "unit_openness",
            heading: "B",
            body: "B body",
        },
    ];
    assert_eq!(
        bundle(&duplicate_bundle_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::DuplicateReference)
    );

    let incomplete_bundle_units = vec![NarrativeUnit {
        interpretation_unit_ref: "unit_openness",
        heading: "A",
        body: "A body",
    }];
    assert_eq!(
        bundle(&incomplete_bundle_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::MissingInterpretationUnit)
    );
}

#[test]
fn empty_selection_and_invalid_references_are_rejected() {
    let units = units();
    let empty = ApprovedStyleSelection {
        interpretation_unit_refs: &[],
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &empty),
        Err(NarrativeFallbackError::EmptySelection)
    );

    let invalid_ref = ApprovedStyleSelection {
        primary_style_ref: "12345",
        ..selection()
    };
    assert_eq!(
        bundle(&units).render(&identity(), &invalid_ref),
        Err(NarrativeFallbackError::InvalidReference)
    );
}

#[test]
fn invalid_display_text_is_rejected() {
    let invalid_units = vec![NarrativeUnit {
        interpretation_unit_ref: "unit_openness",
        heading: " ",
        body: "A body",
    }];
    let selection = ApprovedStyleSelection {
        interpretation_unit_refs: &["unit_openness"],
        ..selection()
    };

    assert_eq!(
        bundle(&invalid_units).render(&identity(), &selection),
        Err(NarrativeFallbackError::InvalidText)
    );
}
