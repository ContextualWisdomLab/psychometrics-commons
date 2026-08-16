//! Domain contract for rebuilding a persisted instrument release after restart.
//!
//! After process restart, call [`InstrumentRelease::from_persisted_snapshot`]
//! with the stored manifest, publication state, and creation time. If the
//! reconstructed release is Published, start new sessions on that exact
//! locale, digest, and item set. Do not use this reconstruction to continue
//! a publication-evidence workflow: stored persist rows do not carry event
//! history or bound evidence.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationState,
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
fn persisted_published_release_may_start_sessions_on_the_exact_form() {
    let release = InstrumentRelease::from_persisted_snapshot(
        published_manifest(),
        PublicationState::Published,
        40_000,
    )
    .unwrap();

    assert!(release.accepts_new_sessions());
    assert_eq!(release.state(), PublicationState::Published);
    assert_eq!(release.created_at_unix_ms(), 40_000);
    assert_eq!(release.manifest().locale(), "ko-KR");
    assert_eq!(release.manifest().content_digest(), VALID_DIGEST);
    assert_eq!(
        release.manifest().item_version_refs(),
        ["item_version_001", "item_version_002"]
    );
    assert!(release.events().is_empty());
    assert!(release.publication_evidence().is_none());
}

#[test]
fn persisted_non_published_states_block_new_sessions() {
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
            "{state:?} must not start new sessions after reload"
        );
        assert_eq!(release.state(), state);
    }
}

#[test]
fn persisted_snapshot_rejects_zero_creation_time() {
    assert_eq!(
        InstrumentRelease::from_persisted_snapshot(
            published_manifest(),
            PublicationState::Published,
            0
        )
        .unwrap_err(),
        InstrumentReleaseError::InvalidTimestamp
    );
}

#[test]
fn persisted_published_release_can_suspend_without_inventing_evidence() {
    let mut release = InstrumentRelease::from_persisted_snapshot(
        published_manifest(),
        PublicationState::Published,
        40_000,
    )
    .unwrap();

    assert_eq!(
        release
            .apply_command("suspend_after_reload", PublicationCommand::Suspend, 40_300)
            .unwrap(),
        PublicationState::Suspended
    );
    assert!(!release.accepts_new_sessions());
}

#[test]
fn persisted_suspended_release_cannot_reactivate_without_rebound_evidence() {
    let mut release = InstrumentRelease::from_persisted_snapshot(
        published_manifest(),
        PublicationState::Suspended,
        40_000,
    )
    .unwrap();

    assert_eq!(
        release
            .apply_command(
                "reactivate_after_reload",
                PublicationCommand::Reactivate,
                40_400
            )
            .unwrap_err(),
        InstrumentReleaseError::MissingPublicationEvidence
    );
    assert_eq!(release.state(), PublicationState::Suspended);
}
