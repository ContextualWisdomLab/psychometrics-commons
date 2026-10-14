//! Participant-facing result copy must stay understandable without exposing
//! operational identities or engine internals. Exact machine-readable provenance
//! remains available through the immutable result-export boundary.

use psychometrics_commons_runtime::localized_result_report::{
    LocalizedResultReport, LocalizedResultReportInput,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{ResultExport, ResultExportInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

#[path = "response_support/mod.rs"]
mod response_support;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const EN_PROVENANCE_NOTE: &str = "Your scores are copied from the saved assessment result. A separate data export keeps the exact versions, time, and scoring evidence needed to check how this result was produced. Internal identifiers are not shown here.";
const KO_PROVENANCE_NOTE: &str = "이 점수는 저장된 검사 결과에서 그대로 가져왔습니다. 별도의 데이터 내보내기에는 결과가 어떻게 만들어졌는지 확인할 수 있도록 정확한 버전, 시점, 채점 근거가 보관됩니다. 내부 식별자는 여기에는 표시하지 않습니다.";

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
fn localized_report_keeps_internal_provenance_out_of_participant_copy() {
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

    // Audit/reference identities and render time remain available through typed
    // fields and the machine-readable export boundary, but they are not copy.
    assert_eq!(
        report.result_snapshot_ref(),
        "result_snapshot_participant_copy_v1"
    );
    assert_eq!(report.participant_ref(), "participant_participant_copy_v1");
    assert_eq!(report.rendered_at_unix_ms(), 1_700_000_100_000);

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
    assert!(text.contains(EN_PROVENANCE_NOTE));
}

#[test]
fn shared_human_readable_export_is_also_minimized_but_keeps_typed_provenance() {
    let snapshot = result_snapshot();
    let export = ResultExport::from_snapshot(
        &snapshot,
        ResultExportInput {
            export_ref: "result_export_participant_copy_v1",
            locale: "en-US",
            exported_at_unix_ms: 1_700_000_100_010,
            limitations: &["This result is not a diagnosis."],
        },
    )
    .unwrap();

    assert_eq!(export.participant_ref(), "participant_participant_copy_v1");
    assert_eq!(export.exported_at_unix_ms(), 1_700_000_100_010);
    assert!(export
        .json_document()
        .contains("\"participant_ref\":\"participant_participant_copy_v1\""));
    assert!(export.json_document().contains(ENGINE_DIGEST));

    let text = export.human_readable_report();
    for internal in [
        "result_export_participant_copy_v1",
        "result_snapshot_participant_copy_v1",
        "participant_participant_copy_v1",
        "instrument_version_participant_copy_v1",
        "scoring_version_participant_copy_v1",
        ENGINE_DIGEST,
        "export_ref:",
        "participant_ref:",
        "engine_artifact_digest:",
    ] {
        assert!(
            !text.contains(internal),
            "shared human-readable export leaked internal provenance {internal:?}: {text}"
        );
    }
    assert!(text.contains("Exact versions, time, ownership, and scoring evidence are retained in the machine-readable data export. Internal identifiers are omitted from this human-readable copy."));
}

#[test]
fn korean_report_explains_provenance_without_internal_ids() {
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

    assert_eq!(report.rendered_at_unix_ms(), 1_700_000_100_001);
    assert!(report.text().contains(KO_PROVENANCE_NOTE));
    assert!(!report.text().contains("참조값:"));
    assert!(!report.text().contains(ENGINE_DIGEST));
}
