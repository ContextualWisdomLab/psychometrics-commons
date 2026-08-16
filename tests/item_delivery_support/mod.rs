//! Shared fixtures for item-delivery integration contracts.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

pub const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

pub fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_big_five_ko_v1"),
        "narrative_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        RELEASE_DIGEST,
    )
    .unwrap()
}

pub fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_item_delivery",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_item_delivery",
                "evidence_policy_self_reflection_v1",
                "release_big_five_ko_v1",
                "instrument_version_ko_v1",
                &["item_version_001", "item_version_002"],
                RELEASE_DIGEST,
                "ko-KR",
                "intended_use_self_reflection_v1",
                "assessment_spec_big_five_v1",
                "scoring_big_five_v1",
                "calibration_big_five_v1",
                Some("norm_big_five_ko_v1"),
                "limitations_big_five_v1",
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
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_item_delivery",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

pub fn session_in_state(release: &InstrumentRelease, state: SessionState) -> AssessmentSession {
    let mut session = AssessmentSession::new(
        "session_big_five_001",
        "participant_big_five_001",
        release,
        "ko-KR",
        20_000,
    )
    .unwrap();

    let commands: &[SessionCommand] = match state {
        SessionState::Created => &[],
        SessionState::Active => &[SessionCommand::Activate],
        SessionState::Paused => &[SessionCommand::Activate, SessionCommand::Pause],
        SessionState::Completed => &[SessionCommand::Activate, SessionCommand::Complete],
        SessionState::Scoring => &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
        ],
        SessionState::Scored => &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
        ],
        SessionState::Released => &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
            SessionCommand::Release,
        ],
        SessionState::Expired => &[SessionCommand::Expire],
        SessionState::Cancelled => &[SessionCommand::Cancel],
        SessionState::Invalidated => &[SessionCommand::Invalidate],
        _ => panic!("unsupported future session state in item-delivery fixture"),
    };

    for (index, command) in commands.iter().copied().enumerate() {
        let command_ref = format!("session_command_item_delivery_{}", index + 1);
        session
            .apply_command(&command_ref, (index + 1) as u64, command)
            .unwrap();
    }
    assert_eq!(session.state(), state);
    session
}
