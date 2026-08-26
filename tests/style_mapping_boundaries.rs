//! Numerical-boundary contracts for the versioned Personality Style presentation mapping.
//!
//! These fixtures protect mapping-policy inclusivity and deterministic tie-breaking. They do not
//! establish psychometric cut scores: the values are presentation parameters owned by the exact
//! mapping version, downstream of already-scored Big Five evidence.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};
use psychometrics_commons_runtime::scoring::ScoreObservation;
use psychometrics_commons_runtime::style_mapping::{
    assign_personality_style, STYLE_MAPPING_VERSION_V1,
};

const RULE_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn identity() -> StyleAssignmentIdentity<'static> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_style_boundary_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: STYLE_MAPPING_VERSION_V1,
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
    }
}

fn scored(construct_ref: &str, score: f64, standard_error: f64) -> ScoreObservation {
    ScoreObservation::scored(construct_ref, score, Some(standard_error)).unwrap()
}

fn big_five(extraversion: f64, openness: f64) -> [ScoreObservation; 5] {
    [
        scored("construct_extraversion", extraversion, 0.20),
        scored("construct_agreeableness", 0.0, 0.20),
        scored("construct_conscientiousness", 0.0, 0.20),
        scored("construct_neuroticism", 0.0, 0.20),
        scored("construct_openness", openness, 0.20),
    ]
}

#[test]
fn absolute_expression_threshold_is_inclusive() {
    let exact = assign_personality_style(&identity(), &big_five(0.50, 0.0)).unwrap();
    assert_eq!(exact.primary_style_ref, "style_social_engagement");

    let just_below = assign_personality_style(&identity(), &big_five(0.499_999, 0.0)).unwrap();
    assert_eq!(just_below.primary_style_ref, "style_balanced_profile");
}

#[test]
fn standard_error_expression_boundary_is_inclusive() {
    let standard_error = 0.50;
    let exact_boundary = 1.96 * standard_error;
    let exact = [
        scored("construct_extraversion", exact_boundary, standard_error),
        scored("construct_agreeableness", 0.0, 0.20),
        scored("construct_conscientiousness", 0.0, 0.20),
        scored("construct_neuroticism", 0.0, 0.20),
        scored("construct_openness", 0.0, 0.20),
    ];
    let assigned = assign_personality_style(&identity(), &exact).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_social_engagement");

    let just_below = [
        scored(
            "construct_extraversion",
            exact_boundary - 0.000_001,
            standard_error,
        ),
        scored("construct_agreeableness", 0.0, 0.20),
        scored("construct_conscientiousness", 0.0, 0.20),
        scored("construct_neuroticism", 0.0, 0.20),
        scored("construct_openness", 0.0, 0.20),
    ];
    let assigned = assign_personality_style(&identity(), &just_below).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_balanced_profile");
}

#[test]
fn adjacent_margin_is_inclusive() {
    let exact = assign_personality_style(&identity(), &big_five(1.25, 1.50)).unwrap();
    assert_eq!(exact.primary_style_ref, "style_exploratory_openness");
    assert_eq!(exact.adjacent_style_refs, vec!["style_social_engagement"]);

    let just_outside = assign_personality_style(&identity(), &big_five(1.249, 1.50)).unwrap();
    assert_eq!(just_outside.primary_style_ref, "style_exploratory_openness");
    assert!(just_outside.adjacent_style_refs.is_empty());
}

#[test]
fn equal_absolute_scores_use_stable_construct_ordering() {
    let assigned = assign_personality_style(&identity(), &big_five(1.50, 1.50)).unwrap();

    assert_eq!(assigned.primary_style_ref, "style_social_engagement");
    assert_eq!(
        assigned.adjacent_style_refs,
        vec!["style_exploratory_openness"]
    );
    assert_eq!(
        assigned.interpretation_unit_refs,
        vec!["unit_social_engagement", "unit_exploratory_openness"]
    );
}
