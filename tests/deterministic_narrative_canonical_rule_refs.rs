//! Regression evidence for canonical deterministic-narrative rule references.

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
fn published_rule_references_must_already_be_canonical() {
    let canonical_units = [unit()];

    for narrative_version_ref in [
        " narrative_version_en_v1",
        "narrative_version_en_v1\u{3000}",
    ] {
        let invalid_bundle = DeterministicNarrativeBundle {
            narrative_version_ref,
            ..bundle(&canonical_units)
        };
        assert_eq!(
            invalid_bundle.render(&identity(), &selection()),
            Err(NarrativeFallbackError::InvalidReference)
        );
    }

    let invalid_style_mapping = DeterministicNarrativeBundle {
        style_mapping_version_ref: "style_mapping_version_v1 ",
        ..bundle(&canonical_units)
    };
    assert_eq!(
        invalid_style_mapping.render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidReference)
    );

    let noncanonical_units = [NarrativeUnit {
        interpretation_unit_ref: "\u{3000}unit_openness",
        ..unit()
    }];
    assert_eq!(
        bundle(&noncanonical_units).render(&identity(), &selection()),
        Err(NarrativeFallbackError::InvalidReference)
    );
}
