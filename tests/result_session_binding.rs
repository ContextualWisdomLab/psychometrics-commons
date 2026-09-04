//! Result publication must preserve the authoritative assessment-session owner and release.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseSnapshot, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

#[path = "response_support/mod.rs"]
mod response_support;

use response_support::{active_session, completed_session};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ENGINE_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

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
            "publication_review_result_binding",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_result_binding",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn session(session_ref: &str, participant_ref: &str) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        participant_ref,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn session_in(
    session_ref: &str,
    participant_ref: &str,
    commands: &[SessionCommand],
) -> AssessmentSession {
    let mut session = session(session_ref, participant_ref);
    for (index, command) in commands.iter().copied().enumerate() {
        session
            .apply_command(
                &format!("command_result_binding_{}", index + 1),
                u64::try_from(index).expect("command index fits in u64") + 1,
                command,
            )
            .unwrap();
    }
    session
}

fn publish_error(session: &AssessmentSession) -> ResultSnapshotError {
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, session.instrument_version_ref());
    let result = scoring_result(&request);
    ResultSnapshot::new(
        session,
        &request,
        &result,
        result_input(session.participant_ref()),
    )
    .unwrap_err()
}

fn completed_snapshot(session_ref: &str) -> ResponseSnapshot {
    let active = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&active).unwrap();
    ledger
        .record(
            &active,
            ResponseWrite {
                server_event_ref: "response_event_result_binding",
                client_event_ref: "client_event_result_binding",
                item_version_ref: "item_version_001",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let completed = completed_session(session_ref);
    ledger
        .freeze_as(&completed, "response_snapshot_result_binding")
        .unwrap()
}

fn scoring_request(snapshot: &ResponseSnapshot, instrument_version_ref: &str) -> ScoringRequest {
    ScoringRequest::from_snapshot(
        snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_binding",
            response_snapshot_ref: "response_snapshot_result_binding",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref,
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn scoring_result(request: &ScoringRequest) -> ScoringResult {
    ScoringResult::new(
        "scoring_result_result_binding",
        request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 0.25, Some(0.1)).unwrap()],
    )
    .unwrap()
}

fn result_input(participant_ref: &str) -> ResultSnapshotInput<'_> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_snapshot_result_binding",
        participant_ref,
        narrative_version_ref: "narrative_version_big_five_v1",
        consent_snapshot_refs: &["consent_service_snapshot_v1"],
        created_at_unix_ms: 30_000,
        supersedes_ref: None,
    }
}

#[test]
fn result_snapshot_rejects_participant_rebinding() {
    let session = session("session_result_binding", "participant_authoritative");
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, session.instrument_version_ref());
    let result = scoring_result(&request);

    let error = ResultSnapshot::new(
        &session,
        &request,
        &result,
        result_input("participant_attacker_controlled"),
    )
    .unwrap_err();

    assert_eq!(error, ResultSnapshotError::ParticipantMismatch);
    assert_eq!(
        error.to_string(),
        "result participant does not match the assessment session owner"
    );
}

#[test]
fn result_snapshot_rejects_instrument_version_rebinding() {
    let session = session("session_result_binding", "participant_authoritative");
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, "instrument_version_unrelated");
    let result = scoring_result(&request);

    let error = ResultSnapshot::new(
        &session,
        &request,
        &result,
        result_input(session.participant_ref()),
    )
    .unwrap_err();

    assert_eq!(error, ResultSnapshotError::InstrumentVersionMismatch);
    assert_eq!(
        error.to_string(),
        "scoring request instrument version does not match the assessment session"
    );
}

#[test]
fn result_snapshot_rejects_session_rebinding() {
    let authoritative_session = session("session_authoritative", "participant_authoritative");
    let response_snapshot = completed_snapshot("session_request_other");
    let request = scoring_request(
        &response_snapshot,
        authoritative_session.instrument_version_ref(),
    );
    let result = scoring_result(&request);

    let error = ResultSnapshot::new(
        &authoritative_session,
        &request,
        &result,
        result_input(authoritative_session.participant_ref()),
    )
    .unwrap_err();

    assert_eq!(error, ResultSnapshotError::SessionMismatch);
    assert_eq!(
        error.to_string(),
        "scoring request does not belong to the supplied assessment session"
    );
}

#[test]
fn result_snapshot_rejects_session_that_has_not_begun_scoring() {
    let created = session("session_result_binding", "participant_authoritative");
    assert_eq!(created.state(), SessionState::Created);
    let error = publish_error(&created);
    assert_eq!(error, ResultSnapshotError::SessionNotReadyForResult);
    assert_eq!(
        error.to_string(),
        "result snapshots can be created only after scoring has begun for the assessment session"
    );

    for commands in [
        [SessionCommand::Activate].as_slice(),
        [SessionCommand::Activate, SessionCommand::Pause].as_slice(),
        [SessionCommand::Activate, SessionCommand::Complete].as_slice(),
        [SessionCommand::Expire].as_slice(),
        [SessionCommand::Cancel].as_slice(),
        [SessionCommand::Invalidate].as_slice(),
        [
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
            SessionCommand::Release,
        ]
        .as_slice(),
    ] {
        let session = session_in(
            "session_result_binding",
            "participant_authoritative",
            commands,
        );
        assert_eq!(
            publish_error(&session),
            ResultSnapshotError::SessionNotReadyForResult
        );
    }
}

#[test]
fn result_snapshot_accepts_scoring_and_scored_sessions() {
    let scoring = session_in(
        "session_result_binding",
        "participant_authoritative",
        &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
        ],
    );
    assert_eq!(scoring.state(), SessionState::Scoring);
    let response_snapshot = completed_snapshot(scoring.session_ref());
    let request = scoring_request(&response_snapshot, scoring.instrument_version_ref());
    let result = scoring_result(&request);
    let snapshot = ResultSnapshot::new(
        &scoring,
        &request,
        &result,
        result_input(scoring.participant_ref()),
    )
    .unwrap();
    assert_eq!(snapshot.participant_ref(), scoring.participant_ref());
    assert_eq!(snapshot.session_ref(), scoring.session_ref());
    assert_eq!(
        snapshot.instrument_version_ref(),
        scoring.instrument_version_ref()
    );

    let scored = session_in(
        "session_result_binding",
        "participant_authoritative",
        &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
            SessionCommand::RecordScore,
        ],
    );
    assert_eq!(scored.state(), SessionState::Scored);
    let snapshot = ResultSnapshot::new(
        &scored,
        &request,
        &result,
        result_input(scored.participant_ref()),
    )
    .unwrap();
    assert_eq!(snapshot.session_ref(), scored.session_ref());
}

#[test]
fn result_snapshot_copies_authoritative_session_provenance() {
    let session = session_in(
        "session_result_binding",
        "participant_authoritative",
        &[
            SessionCommand::Activate,
            SessionCommand::Complete,
            SessionCommand::BeginScoring,
        ],
    );
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, session.instrument_version_ref());
    let result = scoring_result(&request);

    let snapshot = ResultSnapshot::new(
        &session,
        &request,
        &result,
        result_input(session.participant_ref()),
    )
    .unwrap();

    assert_eq!(snapshot.participant_ref(), session.participant_ref());
    assert_eq!(snapshot.session_ref(), session.session_ref());
    assert_eq!(
        snapshot.instrument_version_ref(),
        session.instrument_version_ref()
    );
}
