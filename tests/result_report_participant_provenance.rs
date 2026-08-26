//! Participant-facing result copy must stay understandable without exposing
//! operational identities or engine internals. Exact machine-readable provenance
//! remains available through the immutable result-export boundary.

use psychometrics_commons_runtime::localized_result_report::{
    LocalizedResultReport, LocalizedResultReportInput,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

#[path = "response_support/mod.rs"]
mod response_support;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn result_snapshot() -> ResultSnapshot {
    let mut session = response_support::active_session("session_participant_copy_v1");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    ledger
        .record(
            &session,
            ResponseWrite {
                server_event_ref: "event_participant_copy_alpha",
                client_event_ref: "client_participant_copy_alpha",
                item_version_ref: "item_version_participant_copy_alpha",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    response_support::advance_to(&mut session, SessionState::Completed);
    let response_snapshot = ledger
        .freeze_as(&session, "response_snapshot_participant_copy_v1")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_participant_copy_v1",
            response_snapshot_ref: "response_snapshot_participant_copy_v1",
            assessment_spec_ref: "assessment_spec_participant_copy_v1",
            instrument_version_ref: "instrument_version_participant_copy_v1",
            scoring_version_ref: "scoring_version_participant_copy_v1",
            calibration_reference: "calibration_participant_copy_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_participant_copy_v1",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_extraversion", 0.42, Some(0.18)).unwrap()],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_participant_copy_v1",
            participant_ref: "participant_participant_copy_v1",
            narrative_version_ref: "narrative_version_participant_copy_v1",
            consent_snapshot_refs: &["consent_service_participant_copy_v1"],
            created_at_unix_ms: 1_700_000_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

#[test]
fn human_readable_report_keeps_internal_provenance_out_of_participant_copy() {
    let snapshot = result_snapshot();
    let report = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_participant_copy_v1",
            locale: "en-US",
            rendered_at_unix_ms: 1_700_000_100_000,
            limitations: &["This result is not a diagnosis."],
        },
    )
    .unwrap();

    // Audit/reference identities remain available through typed fields and the
    // machine-readable export boundary, but they are not participant copy.
    assert_eq!(
        report.result_snapshot_ref(),
        "result_snapshot_participant_copy_v1"
    );
    assert_eq!(report.participant_ref(), "participant_participant_copy_v1");

    let text = report.text();
    for internal in [
        "localized_report_participant_copy_v1",
        "result_snapshot_participant_copy_v1",
        "participant_participant_copy_v1",
        "scoring_result_participant_copy_v1",
        "session_participant_copy_v1",
        "response_snapshot_participant_copy_v1",
        "assessment_spec_participant_copy_v1",
        "instrument_version_participant_copy_v1",
        "scoring_version_participant_copy_v1",
        "calibration_participant_copy_v1",
        "narrative_version_participant_copy_v1",
        "consent_service_participant_copy_v1",
        ENGINE_DIGEST,
        "requested_output_schema_version",
        "Unix ms",
    ] {
        assert!(
            !text.contains(internal),
            "participant copy leaked internal provenance {internal:?}: {text}"
        );
    }
    assert!(
        text.contains("Technical provenance is available in the machine-readable result export.")
    );
}

#[test]
fn korean_report_explains_where_to_find_provenance_without_internal_ids() {
    let snapshot = result_snapshot();
    let report = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_participant_copy_ko_v1",
            locale: "ko-KR",
            rendered_at_unix_ms: 1_700_000_100_001,
            limitations: &["이 결과는 진단이 아닙니다."],
        },
    )
    .unwrap();

    assert!(report
        .text()
        .contains("기술 계보는 기계 판독 가능한 결과 내보내기에서 확인할 수 있습니다."));
    assert!(!report.text().contains("참조값:"));
    assert!(!report.text().contains(ENGINE_DIGEST));
}
