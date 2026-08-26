//! Aggregate-level lifecycle contract for release-bound assessment sessions.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionCreationError, SessionState,
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
            "publication_review_11d5b1e7",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_20f6c2a8",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

#[test]
fn aggregate_applies_session_commands_and_preserves_release_provenance() {
    let release = published_release();
    let mut session = AssessmentSession::new(
        "ses_3d657ef743a54698868e4b6ee6c49af4",
        "ptc_471a8fd35e1747b7b25b66d219ce4ccd",
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();
    assert_eq!(session.next_command_sequence(), 1);

    assert_eq!(
        session
            .apply_command(
                "cmd_7e39ee81534f40288d3154b149936170",
                1,
                SessionCommand::Activate,
            )
            .unwrap(),
        SessionState::Active
    );
    assert_eq!(
        session
            .apply_command(
                "cmd_d0b706bf38f44112b5151ccac9da77f1",
                2,
                SessionCommand::Pause,
            )
            .unwrap(),
        SessionState::Paused
    );
    assert_eq!(session.state(), SessionState::Paused);
    assert_eq!(session.next_command_sequence(), 3);
    assert_eq!(
        session
            .apply_client_command(
                "cmd_7e39ee81534f40288d3154b149936170",
                SessionCommand::Activate
            )
            .unwrap(),
        (SessionState::Active, 1)
    );
    assert_eq!(session.state(), SessionState::Paused);
    assert!(session
        .apply_client_command(
            "cmd_7e39ee81534f40288d3154b149936170",
            SessionCommand::Pause
        )
        .is_err());
    assert_eq!(session.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(session.instrument_release_content_digest(), RELEASE_DIGEST);
    assert_eq!(session.locale(), "ko-KR");

    let commands = session.accepted_commands();
    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0].command_ref(),
        "cmd_7e39ee81534f40288d3154b149936170"
    );
    assert_eq!(commands[0].sequence(), 1);
    assert_eq!(commands[0].command(), SessionCommand::Activate);
    assert_eq!(commands[0].resulting_state(), SessionState::Active);
    assert_eq!(
        commands[1].command_ref(),
        "cmd_d0b706bf38f44112b5151ccac9da77f1"
    );
    assert_eq!(commands[1].sequence(), 2);
    assert_eq!(commands[1].command(), SessionCommand::Pause);
    assert_eq!(commands[1].resulting_state(), SessionState::Paused);
}

#[test]
fn session_creation_rejects_whitespace_padded_identity_aliases() {
    let release = published_release();
    let invalid_refs = [
        " session_alpha",
        "session_alpha ",
        "\u{00A0}session_alpha",
        "session_alpha\u{2003}",
        "\u{202F}session_alpha",
        "session_alpha\u{3000}",
    ];

    for invalid_ref in invalid_refs {
        assert_eq!(
            AssessmentSession::new(invalid_ref, "participant_alpha", &release, "ko-KR", 20_000),
            Err(SessionCreationError::InvalidReference),
            "padded session reference must fail closed: {invalid_ref:?}",
        );
        assert_eq!(
            AssessmentSession::new("session_alpha", invalid_ref, &release, "ko-KR", 20_000),
            Err(SessionCreationError::InvalidReference),
            "padded participant reference must fail closed: {invalid_ref:?}",
        );
        assert_eq!(
            AssessmentSession::from_currently_published_manifest(
                invalid_ref,
                "participant_alpha",
                release.manifest(),
                "ko-KR",
                20_000,
            ),
            Err(SessionCreationError::InvalidReference),
            "stored-release session start must reject a padded session reference: {invalid_ref:?}",
        );
        assert_eq!(
            AssessmentSession::from_currently_published_manifest(
                "session_alpha",
                invalid_ref,
                release.manifest(),
                "ko-KR",
                20_000,
            ),
            Err(SessionCreationError::InvalidReference),
            "stored-release session start must reject a padded participant reference: {invalid_ref:?}",
        );
    }
}
