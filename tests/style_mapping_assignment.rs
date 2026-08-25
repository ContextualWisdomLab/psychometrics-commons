//! Contract tests for the first versioned Personality Style presentation mapping.
//!
//! These tests feed already-scored Big Five observations with known values and check
//! that the mapping returns the expected presentation style. They do not estimate
//! latent traits or reimplement fast-mlsirm scoring.

use psychometrics_commons_runtime::deterministic_narrative::{
    DeterministicNarrativeBundle, NarrativeUnit,
};
use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};
use psychometrics_commons_runtime::scoring::{ObservationDisposition, ScoreObservation};
use psychometrics_commons_runtime::style_mapping::{
    assign_personality_style, StyleMappingError, STYLE_MAPPING_VERSION_V1,
};

const RULE_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn identity() -> StyleAssignmentIdentity<'static> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_style_mapping_alpha"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: STYLE_MAPPING_VERSION_V1,
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
    }
}

fn scored(construct_ref: &str, score: f64, standard_error: Option<f64>) -> ScoreObservation {
    ScoreObservation::scored(construct_ref, score, standard_error).unwrap()
}

fn big_five(
    extraversion: f64,
    agreeableness: f64,
    conscientiousness: f64,
    neuroticism: f64,
    openness: f64,
) -> [ScoreObservation; 5] {
    [
        scored("construct_extraversion", extraversion, Some(0.20)),
        scored("construct_agreeableness", agreeableness, Some(0.20)),
        scored("construct_conscientiousness", conscientiousness, Some(0.20)),
        scored("construct_neuroticism", neuroticism, Some(0.20)),
        scored("construct_openness", openness, Some(0.20)),
    ]
}

#[test]
fn dominant_extraversion_assigns_social_engagement_and_replays() {
    let observations = big_five(1.80, 0.10, -0.05, 0.00, 0.20);
    let first = assign_personality_style(&identity(), &observations).unwrap();
    let second = assign_personality_style(&identity(), &observations).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.assignment_key, identity().assignment_key().unwrap());
    assert_eq!(first.primary_style_ref, "style_social_engagement");
    assert!(first.adjacent_style_refs.is_empty());
    assert_eq!(
        first.interpretation_unit_refs,
        vec!["unit_social_engagement"]
    );
    assert_eq!(observations[0].score(), Some(1.80));
}

#[test]
fn opposite_extraversion_pole_assigns_reserved_focus() {
    let observations = big_five(-1.80, 0.10, -0.05, 0.00, 0.20);
    let assigned = assign_personality_style(&identity(), &observations).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_reserved_focus");
    assert_eq!(
        assigned.interpretation_unit_refs,
        vec!["unit_reserved_focus"]
    );
}

#[test]
fn remaining_dominant_poles_follow_the_named_construct() {
    let cases = [
        (
            big_five(0.10, 1.70, 0.00, 0.05, -0.10),
            "style_cooperative_regard",
        ),
        (
            big_five(0.10, -1.70, 0.00, 0.05, -0.10),
            "style_independent_challenge",
        ),
        (
            big_five(0.10, 0.00, 1.60, 0.05, -0.10),
            "style_structured_pursuit",
        ),
        (
            big_five(0.10, 0.00, -1.60, 0.05, -0.10),
            "style_flexible_adaptation",
        ),
        (
            big_five(0.10, 0.00, 0.05, 1.90, -0.10),
            "style_affective_sensitivity",
        ),
        (
            big_five(0.10, 0.00, 0.05, -1.90, -0.10),
            "style_even_affect",
        ),
        (
            big_five(0.10, 0.00, 0.05, 0.00, 1.75),
            "style_exploratory_openness",
        ),
        (
            big_five(0.10, 0.00, 0.05, 0.00, -1.75),
            "style_conventional_grounding",
        ),
    ];

    for (observations, expected) in cases {
        let assigned = assign_personality_style(&identity(), &observations).unwrap();
        assert_eq!(assigned.primary_style_ref, expected);
        assert!(assigned.adjacent_style_refs.is_empty());
    }
}

#[test]
fn close_second_dimension_is_adjacent_not_a_forced_single_type() {
    let observations = big_five(1.40, 0.10, 0.00, 0.05, 1.50);
    let assigned = assign_personality_style(&identity(), &observations).unwrap();

    assert_eq!(assigned.primary_style_ref, "style_exploratory_openness");
    assert_eq!(
        assigned.adjacent_style_refs,
        vec!["style_social_engagement"]
    );
    assert_eq!(
        assigned.interpretation_unit_refs,
        vec!["unit_exploratory_openness", "unit_social_engagement"]
    );
}

#[test]
fn near_zero_profile_is_balanced_instead_of_a_forced_category() {
    let observations = big_five(0.20, -0.10, 0.05, 0.00, -0.15);
    let assigned = assign_personality_style(&identity(), &observations).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_balanced_profile");
    assert!(assigned.adjacent_style_refs.is_empty());
    assert_eq!(
        assigned.interpretation_unit_refs,
        vec!["unit_balanced_profile"]
    );
}

#[test]
fn large_standard_error_prevents_an_uncertain_dimension_from_winning() {
    let observations = [
        scored("construct_extraversion", 0.80, Some(1.00)),
        scored("construct_agreeableness", 0.10, Some(0.20)),
        scored("construct_conscientiousness", 0.00, Some(0.20)),
        scored("construct_neuroticism", 0.05, Some(0.20)),
        scored("construct_openness", 1.20, Some(0.20)),
    ];
    let assigned = assign_personality_style(&identity(), &observations).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_exploratory_openness");
}

#[test]
fn missing_standard_error_still_expresses_a_strong_score() {
    let observations = [
        scored("construct_extraversion", 1.10, None),
        scored("construct_agreeableness", 0.10, Some(0.20)),
        scored("construct_conscientiousness", 0.00, Some(0.20)),
        scored("construct_neuroticism", 0.05, Some(0.20)),
        scored("construct_openness", 0.20, Some(0.20)),
    ];
    let assigned = assign_personality_style(&identity(), &observations).unwrap();
    assert_eq!(assigned.primary_style_ref, "style_social_engagement");
}

#[test]
fn locale_changes_the_assignment_key_but_not_the_style_ref() {
    let observations = big_five(1.80, 0.10, -0.05, 0.00, 0.20);
    let mut korean = identity();
    korean.locale = "ko-KR";

    let english = assign_personality_style(&identity(), &observations).unwrap();
    let localized = assign_personality_style(&korean, &observations).unwrap();

    assert_eq!(english.primary_style_ref, localized.primary_style_ref);
    assert_ne!(english.assignment_key, localized.assignment_key);
}

#[test]
fn mapping_fails_closed_for_version_identity_and_coverage_errors() {
    let observations = big_five(1.80, 0.10, -0.05, 0.00, 0.20);
    let mut wrong_version = identity();
    wrong_version.style_mapping_version_ref = "style_mapping_version_v2";
    assert_eq!(
        assign_personality_style(&wrong_version, &observations),
        Err(StyleMappingError::UnsupportedMappingVersion)
    );

    let mut invalid_identity = identity();
    invalid_identity.instrument_version_ref = "7";
    assert_eq!(
        assign_personality_style(&invalid_identity, &observations),
        Err(StyleMappingError::InvalidIdentity)
    );

    let missing = &observations[..4];
    assert_eq!(
        assign_personality_style(&identity(), missing),
        Err(StyleMappingError::MissingRequiredConstruct)
    );

    let mut duplicate = observations.to_vec();
    duplicate.push(scored("construct_extraversion", 0.90, Some(0.20)));
    assert_eq!(
        assign_personality_style(&identity(), &duplicate),
        Err(StyleMappingError::DuplicateConstruct)
    );

    let abstained = [
        ScoreObservation::without_score(
            "construct_extraversion",
            ObservationDisposition::Abstained,
        )
        .unwrap(),
        scored("construct_agreeableness", 0.10, Some(0.20)),
        scored("construct_conscientiousness", 0.00, Some(0.20)),
        scored("construct_neuroticism", 0.05, Some(0.20)),
        scored("construct_openness", 0.20, Some(0.20)),
    ];
    assert_eq!(
        assign_personality_style(&identity(), &abstained),
        Err(StyleMappingError::UnscoredConstruct)
    );

    let failed = [
        ScoreObservation::without_score("construct_extraversion", ObservationDisposition::Failed)
            .unwrap(),
        scored("construct_agreeableness", 0.10, Some(0.20)),
        scored("construct_conscientiousness", 0.00, Some(0.20)),
        scored("construct_neuroticism", 0.05, Some(0.20)),
        scored("construct_openness", 0.20, Some(0.20)),
    ];
    assert_eq!(
        assign_personality_style(&identity(), &failed),
        Err(StyleMappingError::UnscoredConstruct)
    );

    let excluded = [
        ScoreObservation::without_score("construct_extraversion", ObservationDisposition::Excluded)
            .unwrap(),
        scored("construct_agreeableness", 0.10, Some(0.20)),
        scored("construct_conscientiousness", 0.00, Some(0.20)),
        scored("construct_neuroticism", 0.05, Some(0.20)),
        scored("construct_openness", 0.20, Some(0.20)),
    ];
    assert_eq!(
        assign_personality_style(&identity(), &excluded),
        Err(StyleMappingError::UnscoredConstruct)
    );
}

#[test]
fn assigned_style_renders_through_the_deterministic_fallback() {
    let observations = big_five(1.80, 0.10, -0.05, 0.00, 0.20);
    let assigned = assign_personality_style(&identity(), &observations).unwrap();
    let units = [NarrativeUnit {
        interpretation_unit_ref: "unit_social_engagement",
        heading: "How you engage",
        body: "Your approved score interpretation describes outgoing engagement without replacing the continuous Extraversion score.",
    }];
    let limitations = ["This presentation is descriptive and is not an MBTI-equivalence claim."];
    let bundle = DeterministicNarrativeBundle {
        narrative_version_ref: "narrative_version_en_v1",
        style_mapping_version_ref: STYLE_MAPPING_VERSION_V1,
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
        units: &units,
        limitations: &limitations,
    };

    let rendered = bundle
        .render(&identity(), &assigned.as_approved_selection())
        .unwrap();
    assert_eq!(rendered.primary_style_ref, "style_social_engagement");
    assert_eq!(rendered.sections[0].heading, "How you engage");
}

#[test]
fn style_mapping_errors_are_stable_and_safe() {
    let cases = [
        (
            StyleMappingError::UnsupportedMappingVersion,
            "personality style mapping version is not supported",
        ),
        (
            StyleMappingError::InvalidIdentity,
            "style-assignment identity is invalid",
        ),
        (
            StyleMappingError::MissingRequiredConstruct,
            "personality style mapping requires all five scored Big Five constructs",
        ),
        (
            StyleMappingError::DuplicateConstruct,
            "personality style mapping rejects duplicate construct observations",
        ),
        (
            StyleMappingError::UnscoredConstruct,
            "personality style mapping requires a finite scored observation for each Big Five construct",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
