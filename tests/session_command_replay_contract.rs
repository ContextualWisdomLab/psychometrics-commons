//! Regression contract for immutable session provenance and command replay.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionState, TransitionErrorKind,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001"],
        RELEASE_DIGEST,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_4ca6f53e",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_5db7046f",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn session() -> AssessmentSession {
    let release = published_release();
    AssessmentSession::new(
        "ses_4f82c6fa9e2d4f0b8a7c3d1e5f6a7b8c",
        "ptc_c38017a4d2694f38b1e65a7c9f0d2e4b",
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap()
}

#[test]
fn session_pins_manifest_digest_and_exact_command_replay_does_not_rewind_state() {
    let mut session = session();
    assert_eq!(session.instrument_release_content_digest(), RELEASE_DIGEST);

    assert_eq!(
        session
            .apply_command(
                "cmd_19a8d6e2f4c14e1f9b7a3c5d8e0f2a4b",
                1,
                SessionCommand::Activate,
            )
            .unwrap(),
        SessionState::Active
    );
    session
        .apply_command(
            "cmd_2ab9e7f305d24f20ac8b4d6e9f1a3b5c",
            2,
            SessionCommand::Pause,
        )
        .unwrap();
    assert_eq!(
        session
            .apply_command(
                "cmd_19a8d6e2f4c14e1f9b7a3c5d8e0f2a4b",
                1,
                SessionCommand::Activate,
            )
            .unwrap(),
        SessionState::Active
    );
    assert_eq!(session.state(), SessionState::Paused);

    let command_conflict = session
        .apply_command(
            "cmd_19a8d6e2f4c14e1f9b7a3c5d8e0f2a4b",
            1,
            SessionCommand::Pause,
        )
        .unwrap_err();
    assert_eq!(command_conflict.kind(), TransitionErrorKind::ConflictingReplay);
    assert_eq!(
        command_conflict.to_string(),
        "session command reference was replayed with conflicting evidence"
    );

    let sequence_conflict = session
        .apply_command(
            "cmd_19a8d6e2f4c14e1f9b7a3c5d8e0f2a4b",
            3,
            SessionCommand::Activate,
        )
        .unwrap_err();
    assert_eq!(sequence_conflict.kind(), TransitionErrorKind::ConflictingReplay);
    assert_eq!(session.state(), SessionState::Paused);
}

#[test]
fn new_commands_require_opaque_identity_and_monotonic_positive_sequence() {
    let mut session = session();

    let invalid_ref = session
        .apply_command("12345", 1, SessionCommand::Activate)
        .unwrap_err();
    assert_eq!(invalid_ref.kind(), TransitionErrorKind::InvalidReference);
    assert_eq!(
        invalid_ref.to_string(),
        "session command reference must be opaque and non-numeric"
    );
    assert_eq!(session.state(), SessionState::Created);

    let zero_sequence = session
        .apply_command(
            "cmd_3087c40c06e5451bb0c13abf87aad5e3",
            0,
            SessionCommand::Activate,
        )
        .unwrap_err();
    assert_eq!(zero_sequence.kind(), TransitionErrorKind::InvalidSequence);
    assert_eq!(
        zero_sequence.to_string(),
        "session command sequence must be positive and strictly increasing"
    );

    session
        .apply_command(
            "cmd_4198d51d17f6462ca1d24bc098bbe6f4",
            2,
            SessionCommand::Activate,
        )
        .unwrap();
    let reused_sequence = session
        .apply_command(
            "cmd_52a9e62e2807473db2e35cd1a9ccf705",
            2,
            SessionCommand::Pause,
        )
        .unwrap_err();
    assert_eq!(reused_sequence.kind(), TransitionErrorKind::InvalidSequence);
    assert_eq!(session.state(), SessionState::Active);
}

#[test]
fn invalid_lifecycle_transition_keeps_command_identity_unaccepted() {
    let mut session = session();
    let invalid = session
        .apply_command(
            "cmd_63ba073f3918484ec3f46de2badf0816",
            1,
            SessionCommand::Release,
        )
        .unwrap_err();
    assert_eq!(invalid.kind(), TransitionErrorKind::InvalidTransition);
    assert_eq!(invalid.state(), SessionState::Created);
    assert_eq!(invalid.command(), SessionCommand::Release);
    assert_eq!(
        invalid.to_string(),
        "command Release is not valid while session is Created"
    );

    assert_eq!(
        session
            .apply_command(
                "cmd_63ba073f3918484ec3f46de2badf0816",
                1,
                SessionCommand::Activate,
            )
            .unwrap(),
        SessionState::Active
    );
}
