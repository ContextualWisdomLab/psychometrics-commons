//! Regression coverage for multi-adjacent Personality Style assignments.
//!
//! The v1 presentation mapping preserves every expressed Big Five pole that is close enough to
//! the dominant pole instead of collapsing a near-tied profile to only one adjacent style.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};
use psychometrics_commons_runtime::scoring::ScoreObservation;
use psychometrics_commons_runtime::style_mapping::{
    assign_personality_style, STYLE_MAPPING_VERSION_V1,
};

const RULE_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn identity() -> StyleAssignmentIdentity<'static> {
    StyleAssignmentIdentity {
        score_identity: ScoreIdentity::ScoreProfileRef("score_profile_style_multi_adjacent"),
        instrument_version_ref: "instrument_version_ipip_big_five_en_v1",
        scoring_version_ref: "scoring_version_big_five_v1",
        norm_version_ref: Some("norm_version_reference_v1"),
        style_mapping_version_ref: STYLE_MAPPING_VERSION_V1,
        interpretation_rule_bundle_digest: RULE_DIGEST,
        locale: "en-US",
    }
}

fn scored(construct_ref: &str, score: f64) -> ScoreObservation {
    ScoreObservation::scored(construct_ref, score, Some(0.20)).unwrap()
}

#[test]
fn every_near_tied_expressed_pole_is_preserved_as_adjacent() {
    let observations = [
        scored("construct_extraversion", 1.40),
        scored("construct_agreeableness", 1.30),
        scored("construct_conscientiousness", 0.10),
        scored("construct_neuroticism", 0.05),
        scored("construct_openness", 1.50),
    ];

    let assigned = assign_personality_style(&identity(), &observations).unwrap();

    assert_eq!(assigned.primary_style_ref, "style_exploratory_openness");
    assert_eq!(
        assigned.adjacent_style_refs,
        vec!["style_social_engagement", "style_cooperative_regard"]
    );
    assert_eq!(
        assigned.interpretation_unit_refs,
        vec![
            "unit_exploratory_openness",
            "unit_social_engagement",
            "unit_cooperative_regard",
        ]
    );
}
