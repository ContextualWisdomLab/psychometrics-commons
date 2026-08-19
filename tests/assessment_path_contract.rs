//! Contract tests for versioned Quick and Deep assessment delivery paths.
//!
//! A path chooses an approved ordered subset of one immutable instrument release. The product
//! preserves the release order and provenance; it does not perform psychometric item selection or
//! scoring here.

use psychometrics_commons_runtime::assessment_path::{
    AssessmentPath, AssessmentPathDefinition, AssessmentPathError,
};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use std::error::Error;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_openness_01", "item_extraversion_01", "item_agreeableness_01"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_ko_v1",
        None,
        "narrative_big_five_ko_v1",
        &["consent_service_assessment_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        DIGEST,
    )
    .unwrap()
}

#[test]
fn quick_path_preserves_release_identity_and_ordered_subset() {
    let release = manifest();
    let path = AssessmentPathDefinition::new(
        AssessmentPath::Quick,
        "assessment_path_policy_big_five_v1",
        &release,
        &["item_openness_01", "item_agreeableness_01"],
    )
    .unwrap();

    assert_eq!(path.path(), AssessmentPath::Quick);
    assert_eq!(path.policy_version_ref(), "assessment_path_policy_big_five_v1");
    assert_eq!(path.release_ref(), "release_big_five_ko_v1");
    assert_eq!(
        path.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(path.locale(), "ko-KR");
    assert_eq!(
        path.item_version_refs(),
        ["item_openness_01", "item_agreeableness_01"]
    );
}

#[test]
fn deep_path_may_use_the_full_published_release_order() {
    let release = manifest();
    let path = AssessmentPathDefinition::new(
        AssessmentPath::Deep,
        "assessment_path_policy_big_five_v1",
        &release,
        &[
            "item_openness_01",
            "item_extraversion_01",
            "item_agreeableness_01",
        ],
    )
    .unwrap();

    assert_eq!(path.path(), AssessmentPath::Deep);
    assert_eq!(path.item_version_refs(), release.item_version_refs());
}

#[test]
fn path_rejects_empty_duplicate_unknown_or_reordered_items() {
    let release = manifest();

    assert_eq!(
        AssessmentPathDefinition::new(
            AssessmentPath::Quick,
            "assessment_path_policy_big_five_v1",
            &release,
            &[],
        )
        .unwrap_err(),
        AssessmentPathError::EmptyItemSet
    );
    assert_eq!(
        AssessmentPathDefinition::new(
            AssessmentPath::Quick,
            "assessment_path_policy_big_five_v1",
            &release,
            &["item_openness_01", "item_openness_01"],
        )
        .unwrap_err(),
        AssessmentPathError::DuplicateItemReference
    );
    assert_eq!(
        AssessmentPathDefinition::new(
            AssessmentPath::Quick,
            "assessment_path_policy_big_five_v1",
            &release,
            &["item_openness_01", "item_neuroticism_01"],
        )
        .unwrap_err(),
        AssessmentPathError::ItemOutsideRelease
    );
    assert_eq!(
        AssessmentPathDefinition::new(
            AssessmentPath::Quick,
            "assessment_path_policy_big_five_v1",
            &release,
            &["item_agreeableness_01", "item_openness_01"],
        )
        .unwrap_err(),
        AssessmentPathError::ItemOrderMismatch
    );
}

#[test]
fn path_policy_reference_must_use_exact_opaque_spelling() {
    let release = manifest();

    for policy_ref in ["", "12345", " assessment_path_policy_big_five_v1 "] {
        let error = AssessmentPathDefinition::new(
            AssessmentPath::Quick,
            policy_ref,
            &release,
            &["item_openness_01"],
        )
        .unwrap_err();
        assert_eq!(error, AssessmentPathError::InvalidReference);
        assert_eq!(
            error.to_string(),
            "assessment path policy reference must be an exact opaque non-numeric value"
        );
        assert!(error.source().is_none());
    }
}

#[test]
fn path_value_semantics_preserve_immutable_delivery_evidence() {
    let release = manifest();
    let primary = AssessmentPathDefinition::new(
        AssessmentPath::Quick,
        "assessment_path_policy_big_five_v1",
        &release,
        &["item_openness_01", "item_extraversion_01"],
    )
    .unwrap();
    let cloned = primary.clone();
    let deep = AssessmentPathDefinition::new(
        AssessmentPath::Deep,
        "assessment_path_policy_big_five_v1",
        &release,
        &["item_openness_01", "item_extraversion_01"],
    )
    .unwrap();

    assert_eq!(primary, cloned);
    assert_ne!(primary, deep);
    assert!(format!("{primary:?}").contains("Quick"));
}
