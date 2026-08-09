//! Contract tests for immutable instrument-release publication and session eligibility.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationState,
};

fn manifest() -> InstrumentReleaseManifest {
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
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap()
}

#[test]
fn manifest_pins_every_release_critical_reference_without_mutators() {
    let manifest = manifest();
    assert_eq!(manifest.release_ref(), "release_big_five_ko_v1");
    assert_eq!(manifest.instrument_ref(), "instrument_big_five");
    assert_eq!(
        manifest.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(manifest.construct_ref(), "construct_big_five");
    assert_eq!(
        manifest.item_version_refs(),
        ["item_version_001", "item_version_002"]
    );
    assert_eq!(manifest.locale(), "ko-KR");
    assert_eq!(
        manifest.assessment_spec_ref(),
        "assessment_spec_big_five_v1"
    );
    assert_eq!(
        manifest.scoring_version_ref(),
        "scoring_version_big_five_v1"
    );
    assert_eq!(
        manifest.calibration_reference(),
        "calibration_big_five_ko_v1"
    );
    assert_eq!(
        manifest.norm_version_ref(),
        Some("norm_version_big_five_ko_v1")
    );
    assert_eq!(
        manifest.narrative_version_ref(),
        "narrative_version_big_five_v1"
    );
    assert_eq!(manifest.consent_requirement_refs(), ["consent_service_v1"]);
    assert_eq!(
        manifest.intended_use_ref(),
        "intended_use_self_reflection_v1"
    );
    assert_eq!(manifest.limitations_ref(), "limitations_nonclinical_v1");
    assert_eq!(
        manifest.content_digest(),
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    );
}

#[test]
fn malformed_release_contracts_fail_closed() {
    let valid_digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let base = |release_ref: &str, items: &[&str], locale: &str, digest: &str| {
        InstrumentReleaseManifest::new(
            release_ref,
            "instrument_big_five",
            "instrument_version_big_five_ko_v1",
            "construct_big_five",
            items,
            locale,
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_ko_v1",
            None,
            "narrative_version_big_five_v1",
            &["consent_service_v1"],
            "intended_use_self_reflection_v1",
            "limitations_nonclinical_v1",
            digest,
        )
    };

    assert_eq!(
        base("12345", &["item_version_001"], "ko-KR", valid_digest),
        Err(InstrumentReleaseError::InvalidReference)
    );
    assert_eq!(
        base("release_ref", &[], "ko-KR", valid_digest),
        Err(InstrumentReleaseError::EmptyItemSet)
    );
    assert_eq!(
        base(
            "release_ref",
            &["item_version_001", "item_version_001"],
            "ko-KR",
            valid_digest,
        ),
        Err(InstrumentReleaseError::DuplicateItemReference)
    );
    assert_eq!(
        base("release_ref", &["item_version_001"], "ko_KR", valid_digest),
        Err(InstrumentReleaseError::InvalidLocale)
    );
    assert_eq!(
        base(
            "release_ref",
            &["item_version_001"],
            "ko-KR",
            "sha256:not-a-digest",
        ),
        Err(InstrumentReleaseError::InvalidDigest)
    );
}

#[test]
fn publication_requires_review_and_controls_new_session_eligibility() {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    assert_eq!(release.state(), PublicationState::Draft);
    assert!(!release.accepts_new_sessions());

    assert_eq!(
        release.apply_command(
            "publish_too_early_event",
            PublicationCommand::Publish,
            10_050,
        ),
        Err(InstrumentReleaseError::InvalidTransition)
    );

    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    assert_eq!(release.state(), PublicationState::Review);

    release
        .apply_command("publish_event", PublicationCommand::Publish, 10_200)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Published);
    assert!(release.accepts_new_sessions());

    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 10_300)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Suspended);
    assert!(!release.accepts_new_sessions());

    release
        .apply_command("reactivate_event", PublicationCommand::Reactivate, 10_400)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Published);
    assert!(release.accepts_new_sessions());

    release
        .apply_command("retire_event", PublicationCommand::Retire, 10_500)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Retired);
    assert!(!release.accepts_new_sessions());
    assert!(release.state().is_terminal());
}

#[test]
fn event_replay_is_idempotent_and_never_reopens_later_state() {
    let mut release = InstrumentRelease::new(manifest(), 20_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            20_100,
        )
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 20_200)
        .unwrap();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 20_300)
        .unwrap();

    release
        .apply_command("publish_event", PublicationCommand::Publish, 20_200)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Suspended);

    assert_eq!(
        release.apply_command("publish_event", PublicationCommand::Publish, 20_201,),
        Err(InstrumentReleaseError::ConflictingReplay)
    );
    assert_eq!(
        release.apply_command("publish_event", PublicationCommand::Retire, 20_200,),
        Err(InstrumentReleaseError::ConflictingReplay)
    );
}

#[test]
fn event_time_is_server_monotonic_and_retirement_is_terminal() {
    let mut release = InstrumentRelease::new(manifest(), 30_000).unwrap();
    assert_eq!(
        release.apply_command("zero_time_event", PublicationCommand::SubmitReview, 0,),
        Err(InstrumentReleaseError::InvalidTimestamp)
    );
    assert_eq!(
        release.apply_command("backward_event", PublicationCommand::SubmitReview, 29_999,),
        Err(InstrumentReleaseError::NonMonotonicTimestamp)
    );

    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            30_100,
        )
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 30_200)
        .unwrap();
    release
        .apply_command("retire_event", PublicationCommand::Retire, 30_300)
        .unwrap();

    for command in [
        PublicationCommand::SubmitReview,
        PublicationCommand::Publish,
        PublicationCommand::Suspend,
        PublicationCommand::Reactivate,
        PublicationCommand::Retire,
    ] {
        assert_eq!(
            release.apply_command("new_terminal_event", command, 30_400),
            Err(InstrumentReleaseError::InvalidTransition)
        );
    }
}
