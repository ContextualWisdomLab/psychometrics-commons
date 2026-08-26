//! Shared published-release fixtures for response-ledger session-authority tests.
#![allow(dead_code)]

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseSnapshot, ResponseWrite};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

const RELEASE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVIDENCE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Return one published locale-specific instrument release for response tests.
#[must_use]
pub fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
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
        RELEASE_DIGEST,
    )
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_response_support",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_response_support",
                "evidence_policy_self_reflection_v1",
                "release_big_five_ko_v1",
                "instrument_version_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
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
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_response_support",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

/// Create a session that has not yet begun accepting responses.
#[must_use]
pub fn created_session(session_ref: &str) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        "participant_response_support",
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

/// Create a session whose authoritative lifecycle matches `target`.
#[must_use]
pub fn session_in_state(session_ref: &str, target: SessionState) -> AssessmentSession {
    let mut session = created_session(session_ref);
    let mut sequence = 0_u64;
    for command in commands_to(target) {
        sequence += 1;
        let command_ref = format!("session_command_{}_{sequence}", session.session_ref());
        session
            .apply_command(&command_ref, sequence, command)
            .unwrap();
    }
    assert_eq!(session.state(), target);
    session
}

/// Create an active session that may accept new response events.
#[must_use]
pub fn active_session(session_ref: &str) -> AssessmentSession {
    session_in_state(session_ref, SessionState::Active)
}

/// Create a completed session that may freeze a response snapshot.
#[must_use]
pub fn completed_session(session_ref: &str) -> AssessmentSession {
    session_in_state(session_ref, SessionState::Completed)
}

/// Advance `session` from its current state to `target` using legal commands.
pub fn advance_to(session: &mut AssessmentSession, target: SessionState) {
    let mut sequence = 0_u64;
    for command in commands_to(target) {
        sequence += 1;
        let command_ref = format!("session_command_{}_{sequence}", session.session_ref());
        session
            .apply_command(&command_ref, sequence, command)
            .unwrap();
    }
    assert_eq!(session.state(), target);
}

/// Record writes against an active session and freeze a completed snapshot.
#[must_use]
pub fn frozen_snapshot(
    session_ref: &str,
    snapshot_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> ResponseSnapshot {
    let mut session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    for request in writes {
        ledger.record(&session, *request).unwrap();
    }
    advance_to(&mut session, SessionState::Completed);
    ledger.freeze_as(&session, snapshot_ref).unwrap()
}

/// Record writes against an active session and freeze an unbound completed snapshot.
#[must_use]
pub fn unbound_frozen_snapshot(
    session_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> ResponseSnapshot {
    let mut session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    for request in writes {
        ledger.record(&session, *request).unwrap();
    }
    advance_to(&mut session, SessionState::Completed);
    ledger.freeze(&session).unwrap()
}

fn commands_to(target: SessionState) -> Vec<SessionCommand> {
    match target {
        SessionState::Created => Vec::new(),
        SessionState::Active => vec![SessionCommand::Activate],
        SessionState::Paused => vec![SessionCommand::Activate, SessionCommand::Pause],
        SessionState::Completed => vec![SessionCommand::Activate, SessionCommand::Complete],
        SessionState::Scoring => vec![
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
        ],
        SessionState::Scored => vec![
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
        ],
        SessionState::Released => vec![
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
            SessionCommand::Release,
        ],
        SessionState::Expired => vec![SessionCommand::Expire],
        SessionState::Cancelled => vec![SessionCommand::Cancel],
        SessionState::Invalidated => vec![SessionCommand::Invalidate],
        other => panic!("response fixtures do not construct {other:?}"),
    }
}
