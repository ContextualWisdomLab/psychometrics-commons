//! Contract tests for immutable instrument-release publication and session eligibility.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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
        VALID_DIGEST,
    )
    .unwrap()
}

fn custom_manifest(
    release_ref: &str,
    items: &[&str],
    locale: &str,
    norm_version_ref: Option<&str>,
    consent_requirement_refs: &[&str],
    digest: &str,
) -> Result<InstrumentReleaseManifest, InstrumentReleaseError> {
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
        norm_version_ref,
        "narrative_version_big_five_v1",
        consent_requirement_refs,
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        digest,
    )
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
    assert_eq!(manifest.content_digest(), VALID_DIGEST);
}

#[test]
fn optional_norm_and_consent_requirements_remain_explicitly_absent() {
    let manifest = custom_manifest(
        "release_big_five_en_v1",
        &["item_version_001"],
        " en ",
        None,
        &[],
        VALID_DIGEST,
    )
    .unwrap();

    assert_eq!(manifest.locale(), "en");
    assert_eq!(manifest.norm_version_ref(), None);
    assert!(manifest.consent_requirement_refs().is_empty());
}

#[test]
fn malformed_release_contracts_fail_closed() {
    assert_eq!(
        custom_manifest(
            "12345",
            &["item_version_001"],
            "ko-KR",
            None,
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &[],
            "ko-KR",
            None,
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::EmptyItemSet)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &["item_version_001", "item_version_001"],
            "ko-KR",
            None,
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::DuplicateItemReference)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &["item_version_001"],
            "ko_KR",
            None,
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::InvalidLocale)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &["item_version_001"],
            "ko-KR",
            None,
            &["consent_service_v1"],
            "sha256:not-a-digest",
        ),
        Err(InstrumentReleaseError::InvalidDigest)
    );
}

#[test]
fn locale_digest_and_reference_edge_cases_fail_closed() {
    for locale in ["", "k", "languagexx", "ko-", "ko-ABCDEFGHI", "ko-KR!"] {
        assert_eq!(
            custom_manifest(
                "release_ref",
                &["item_version_001"],
                locale,
                None,
                &["consent_service_v1"],
                VALID_DIGEST,
            ),
            Err(InstrumentReleaseError::InvalidLocale),
            "locale {locale:?} must fail closed"
        );
    }

    let uppercase_digest = format!("sha256:{}A", "0".repeat(63));
    for digest in [
        "md5:0123456789abcdef0123456789abcdef",
        "sha256:0123",
        uppercase_digest.as_str(),
    ] {
        assert_eq!(
            custom_manifest(
                "release_ref",
                &["item_version_001"],
                "ko-KR",
                None,
                &["consent_service_v1"],
                digest,
            ),
            Err(InstrumentReleaseError::InvalidDigest),
            "digest {digest:?} must fail closed"
        );
    }

    assert_eq!(
        custom_manifest(
            "release_ref",
            &["item_version_001"],
            "ko-KR",
            Some("12345"),
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &["item_version_001"],
            "ko-KR",
            None,
            &["consent_service_v1", "consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
    assert_eq!(
        custom_manifest(
            "release_ref",
            &["12345"],
            "ko-KR",
            None,
            &["consent_service_v1"],
            VALID_DIGEST,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
}

#[test]
fn publication_requires_review_and_controls_new_session_eligibility() {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    assert_eq!(release.state(), PublicationState::Draft);
    assert!(!release.accepts_new_sessions());
    assert!(!release.state().is_terminal());

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
fn release_metadata_and_publication_events_are_auditable() {
    let expected_manifest = manifest();
    let mut release = InstrumentRelease::new(expected_manifest.clone(), 15_000).unwrap();

    assert_eq!(release.manifest(), &expected_manifest);
    assert_eq!(release.created_at_unix_ms(), 15_000);
    assert!(release.events().is_empty());

    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            15_100,
        )
        .unwrap();
    let event = &release.events()[0];
    assert_eq!(event.event_ref(), "submit_review_event");
    assert_eq!(event.command(), PublicationCommand::SubmitReview);
    assert_eq!(event.occurred_at_unix_ms(), 15_100);
}

#[test]
fn release_rejects_zero_creation_time_and_invalid_event_reference() {
    assert_eq!(
        InstrumentRelease::new(manifest(), 0),
        Err(InstrumentReleaseError::InvalidTimestamp)
    );

    let mut release = InstrumentRelease::new(manifest(), 17_000).unwrap();
    assert_eq!(
        release.apply_command("12345", PublicationCommand::SubmitReview, 17_100),
        Err(InstrumentReleaseError::InvalidReference)
    );
}

#[test]
fn suspended_release_can_retire_without_reactivation() {
    let mut release = InstrumentRelease::new(manifest(), 18_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            18_100,
        )
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 18_200)
        .unwrap();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 18_300)
        .unwrap();
    assert_eq!(
        release
            .apply_command("retire_event", PublicationCommand::Retire, 18_400)
            .unwrap(),
        PublicationState::Retired
    );
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
        release.apply_command("publish_event", PublicationCommand::Publish, 20_201),
        Err(InstrumentReleaseError::ConflictingReplay)
    );
    assert_eq!(
        release.apply_command("publish_event", PublicationCommand::Retire, 20_200),
        Err(InstrumentReleaseError::ConflictingReplay)
    );
}

#[test]
fn event_time_is_server_monotonic_and_retirement_is_terminal() {
    let mut release = InstrumentRelease::new(manifest(), 30_000).unwrap();
    assert_eq!(
        release.apply_command("zero_time_event", PublicationCommand::SubmitReview, 0),
        Err(InstrumentReleaseError::InvalidTimestamp)
    );
    assert_eq!(
        release.apply_command("backward_event", PublicationCommand::SubmitReview, 29_999),
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

#[test]
fn instrument_release_errors_have_stable_safe_display_text() {
    let cases = [
        (
            InstrumentReleaseError::InvalidReference,
            "instrument release references must be opaque non-numeric values",
        ),
        (
            InstrumentReleaseError::EmptyItemSet,
            "instrument release must contain at least one item version",
        ),
        (
            InstrumentReleaseError::DuplicateItemReference,
            "instrument release item-version references must be unique",
        ),
        (
            InstrumentReleaseError::InvalidLocale,
            "instrument release locale must be a valid BCP 47-style tag",
        ),
        (
            InstrumentReleaseError::InvalidDigest,
            "instrument release content digest must be sha256 followed by 64 lowercase hexadecimal digits",
        ),
        (
            InstrumentReleaseError::InvalidTimestamp,
            "instrument publication timestamps must be greater than zero",
        ),
        (
            InstrumentReleaseError::NonMonotonicTimestamp,
            "instrument publication event time must not move backwards",
        ),
        (
            InstrumentReleaseError::ConflictingReplay,
            "instrument publication event reference was replayed with conflicting evidence",
        ),
        (
            InstrumentReleaseError::InvalidTransition,
            "instrument publication command is not allowed from the current state",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn cloned_publication_evidence_preserves_immutable_identity_and_state() {
    let mut release = InstrumentRelease::new(manifest(), 40_000).unwrap();
    release
        .apply_command(
            "submit_review_clone_event",
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();

    let cloned_manifest = release.manifest().clone();
    let cloned_event = release.events()[0].clone();
    let cloned_release = release.clone();

    assert_eq!(cloned_manifest, *release.manifest());
    assert_eq!(cloned_event, release.events()[0]);
    assert_eq!(cloned_release, release);
    assert_eq!(
        PublicationState::clone(&PublicationState::Review),
        PublicationState::Review
    );
    assert_eq!(
        PublicationCommand::clone(&PublicationCommand::SubmitReview),
        PublicationCommand::SubmitReview
    );
    assert_eq!(
        InstrumentReleaseError::clone(&InstrumentReleaseError::InvalidReference),
        InstrumentReleaseError::InvalidReference
    );
    assert!(format!("{cloned_release:?}").contains("InstrumentRelease"));
}
