//! Domain eligibility for a startable catalog entry.
//!
//! A reconstructed release is catalog-startable only when it is Published.
//! The `PostgreSQL` adapter list lives in
//! `postgres_instrument_release_persistence`. Copy the `release_ref` and
//! exact BCP 47 `locale` into the #205 session-start path. Do not invent a
//! fallback locale.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn published_manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        VALID_DIGEST,
    )
    .unwrap()
}

#[test]
fn only_published_reconstructions_are_catalog_startable() {
    let published = InstrumentRelease::from_persisted_snapshot(
        published_manifest(),
        PublicationState::Published,
        40_000,
    )
    .unwrap();
    assert!(published.accepts_new_sessions());
    assert_eq!(published.manifest().release_ref(), "release_big_five_ko_v1");
    assert_eq!(published.manifest().locale(), "ko-KR");

    for state in [
        PublicationState::Draft,
        PublicationState::Review,
        PublicationState::Suspended,
        PublicationState::Retired,
    ] {
        let release =
            InstrumentRelease::from_persisted_snapshot(published_manifest(), state, 40_000)
                .unwrap();
        assert!(
            !release.accepts_new_sessions(),
            "{state:?} must stay hidden from the startable catalog"
        );
    }
}
