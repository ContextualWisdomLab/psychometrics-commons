//! Exact-reference identity evidence for Personality Style assignment.

use psychometrics_commons_runtime::narrative::{ScoreIdentity, StyleAssignmentIdentity};
use psychometrics_commons_runtime::scoring::ScoreObservation;
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

fn observations() -> [ScoreObservation; 5] {
    [
        ScoreObservation::scored("construct_extraversion", 1.80, Some(0.20)).unwrap(),
        ScoreObservation::scored("construct_agreeableness", 0.10, Some(0.20)).unwrap(),
        ScoreObservation::scored("construct_conscientiousness", -0.05, Some(0.20)).unwrap(),
        ScoreObservation::scored("construct_neuroticism", 0.00, Some(0.20)).unwrap(),
        ScoreObservation::scored("construct_openness", 0.20, Some(0.20)).unwrap(),
    ]
}

#[test]
fn mapping_rejects_noncanonical_assignment_reference_aliases() {
    let aliases = [
        StyleAssignmentIdentity {
            score_identity: ScoreIdentity::ScoreProfileRef(" score_profile_style_mapping_alpha "),
            ..identity()
        },
        StyleAssignmentIdentity {
            instrument_version_ref: " instrument_version_ipip_big_five_en_v1 ",
            ..identity()
        },
        StyleAssignmentIdentity {
            scoring_version_ref: " scoring_version_big_five_v1 ",
            ..identity()
        },
        StyleAssignmentIdentity {
            norm_version_ref: Some(" norm_version_reference_v1 "),
            ..identity()
        },
        StyleAssignmentIdentity {
            style_mapping_version_ref: " style_mapping_version_v1 ",
            ..identity()
        },
    ];
    let observations = observations();

    for alias in aliases {
        assert!(alias.assignment_key().is_ok());
        assert_eq!(
            assign_personality_style(&alias, &observations),
            Err(StyleMappingError::InvalidIdentity)
        );
    }
}

#[test]
fn mapping_accepts_canonical_inline_score_digest_identity() {
    let digest_identity = StyleAssignmentIdentity {
        score_identity: ScoreIdentity::CanonicalScorePayloadDigest(RULE_DIGEST),
        ..identity()
    };
    let assigned = assign_personality_style(&digest_identity, &observations()).unwrap();

    assert_eq!(
        assigned.assignment_key,
        digest_identity.assignment_key().unwrap()
    );
}
