//! Result publication must preserve the authoritative assessment-session owner and release.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseSnapshot, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionState};

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

fn session(participant_ref: &str) -> AssessmentSession {
    AssessmentSession::new(
        "session_result_binding",
        participant_ref,
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn completed_snapshot(session_ref: &str) -> ResponseSnapshot {
    let mut ledger = ResponseLedger::new(session_ref).unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_result_binding",
                client_event_ref: "client_event_result_binding",
                item_version_ref: "item_version_001",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "response_snapshot_result_binding")
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
fn result_snapshot_cannot_rebind_a_session_to_another_participant() {
    let session = session("participant_authoritative");
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, session.instrument_version_ref());
    let result = scoring_result(&request);

    let snapshot = ResultSnapshot::new(
        &request,
        &result,
        result_input("participant_attacker_controlled"),
    )
    .unwrap();

    assert_eq!(snapshot.participant_ref(), session.participant_ref());
}

#[test]
fn result_snapshot_cannot_rebind_a_session_to_another_instrument_version() {
    let session = session("participant_authoritative");
    let response_snapshot = completed_snapshot(session.session_ref());
    let request = scoring_request(&response_snapshot, "instrument_version_unrelated");
    let result = scoring_result(&request);

    let snapshot = ResultSnapshot::new(
        &request,
        &result,
        result_input(session.participant_ref()),
    )
    .unwrap();

    assert_eq!(
        snapshot.instrument_version_ref(),
        session.instrument_version_ref()
    );
}
