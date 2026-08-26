//! Localized personal reports preserve immutable scores while rendering only
//! explicitly supported participant-facing locales.

use psychometrics_commons_runtime::localized_result_report::{
    LocalizedResultReport, LocalizedResultReportError, LocalizedResultReportInput,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;
use std::error::Error;

#[path = "response_support/mod.rs"]
mod response_support;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn result_snapshot() -> ResultSnapshot {
    let mut session = response_support::active_session("session_big_five_locale_v1");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    ledger
        .record(
            &session,
            ResponseWrite {
                server_event_ref: "event_locale_item_alpha",
                client_event_ref: "client_locale_item_alpha",
                item_version_ref: "item_version_locale_alpha",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    response_support::advance_to(&mut session, SessionState::Completed);
    let response_snapshot = ledger
        .freeze_as(&session, "response_snapshot_locale_v1")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_locale_v1",
            response_snapshot_ref: "response_snapshot_locale_v1",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_locale_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_locale_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_locale_v1",
        &request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored("construct_extraversion", 0.42, Some(0.18)).unwrap(),
            ScoreObservation::scored("construct_conscientiousness", 0.67, None).unwrap(),
            ScoreObservation::without_score(
                "construct_openness",
                ObservationDisposition::Abstained,
            )
            .unwrap(),
            ScoreObservation::without_score(
                "construct_agreeableness",
                ObservationDisposition::Failed,
            )
            .unwrap(),
            ScoreObservation::without_score(
                "construct_neuroticism",
                ObservationDisposition::Excluded,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_locale_v1",
            participant_ref: "participant_locale_alpha",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_service_locale_v1"],
            created_at_unix_ms: 1_700_000_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

#[test]
fn korean_report_uses_korean_structure_without_mutating_scores() {
    let snapshot = result_snapshot();
    let report = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_ko_v1",
            locale: "ko-KR",
            rendered_at_unix_ms: 1_700_000_100_000,
            limitations: &["이 결과는 진단 또는 채용 적격 판정이 아닙니다."],
        },
    )
    .unwrap();
    let retained = report.clone();

    assert_eq!(retained, report);
    assert!(format!("{report:?}").contains("localized_report_ko_v1"));
    assert_eq!(report.report_ref(), "localized_report_ko_v1");
    assert_eq!(report.participant_ref(), "participant_locale_alpha");
    assert_eq!(report.locale(), "ko-KR");
    assert_eq!(report.result_snapshot_ref(), "result_snapshot_locale_v1");
    assert!(report.text().starts_with("개인 결과 보고서\n"));
    assert!(report
        .text()
        .contains("기술 계보는 기계 판독 가능한 결과 내보내기에서 확인할 수 있습니다."));
    assert!(report.text().contains("\n점수\n"));
    assert!(report.text().contains("\n제한사항\n"));
    assert!(report
        .text()
        .contains("construct_extraversion: 채점됨 0.42 (표준오차 0.18)"));
    assert!(report
        .text()
        .contains("construct_conscientiousness: 채점됨 0.67"));
    assert!(report.text().contains("construct_openness: 보류"));
    assert!(report.text().contains("construct_agreeableness: 실패"));
    assert!(report.text().contains("construct_neuroticism: 제외"));
    assert!(report
        .text()
        .contains("이 결과는 진단 또는 채용 적격 판정이 아닙니다."));
    assert_eq!(snapshot.score_observations()[0].score(), Some(0.42));
}

#[test]
fn localized_report_keeps_auditable_provenance_outside_human_readable_copy() {
    let snapshot = result_snapshot();
    let report = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_provenance_v1",
            locale: "ko-KR",
            rendered_at_unix_ms: 1_700_000_100_004,
            limitations: &["이 결과는 진단 또는 채용 적격 판정이 아닙니다."],
        },
    )
    .unwrap();

    assert_eq!(report.report_ref(), "localized_report_provenance_v1");
    assert_eq!(report.result_snapshot_ref(), "result_snapshot_locale_v1");
    assert_eq!(report.participant_ref(), "participant_locale_alpha");
    assert_eq!(report.locale(), "ko-KR");

    for internal in [
        "localized_report_provenance_v1",
        "result_snapshot_locale_v1",
        "participant_locale_alpha",
        "session_big_five_locale_v1",
        "response_snapshot_locale_v1",
        "assessment_spec_big_five_v1",
        "instrument_version_big_five_locale_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_locale_v1",
        "narrative_version_big_five_v1",
        "consent_service_locale_v1",
        ENGINE_DIGEST,
        "출력 스키마 버전",
        "Unix ms",
        "참조값:",
    ] {
        assert!(
            !report.text().contains(internal),
            "participant copy leaked internal provenance {internal:?}: {}",
            report.text()
        );
    }
}

#[test]
fn english_report_remains_explicitly_english() {
    let snapshot = result_snapshot();
    let report = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_en_v1",
            locale: "en-US",
            rendered_at_unix_ms: 1_700_000_100_001,
            limitations: &["This result is not a diagnosis or employment-fitness decision."],
        },
    )
    .unwrap();

    assert!(report.text().starts_with("Personal result report\n"));
    assert!(report
        .text()
        .contains("Technical provenance is available in the machine-readable result export."));
    assert!(report.text().contains("\nScores\n"));
    assert!(report.text().contains("\nLimitations\n"));
    assert!(report
        .text()
        .contains("construct_extraversion: scored 0.42 (SE 0.18)"));
    assert!(report
        .text()
        .contains("construct_conscientiousness: scored 0.67"));
    assert!(report.text().contains("construct_openness: abstained"));
    assert!(report.text().contains("construct_agreeableness: failed"));
    assert!(report.text().contains("construct_neuroticism: excluded"));
}

#[test]
fn unsupported_or_noncanonical_locale_fails_closed() {
    let snapshot = result_snapshot();
    for locale in ["fr-FR", "ko", "en", "ko-kr", " ko-KR", "en-US "] {
        let error = LocalizedResultReport::from_snapshot(
            &snapshot,
            LocalizedResultReportInput {
                report_ref: "localized_report_invalid_v1",
                locale,
                rendered_at_unix_ms: 1_700_000_100_002,
                limitations: &["Reviewed limitation."],
            },
        )
        .unwrap_err();
        assert_eq!(error, LocalizedResultReportError::UnsupportedLocale);
        assert!(error.to_string().contains("ko-KR or en-US"));
        assert!(error.source().is_none());
    }
}

#[test]
fn invalid_export_input_preserves_underlying_error_source() {
    let snapshot = result_snapshot();
    let error = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: " ",
            locale: "en-US",
            rendered_at_unix_ms: 1_700_000_100_003,
            limitations: &["Reviewed limitation."],
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        LocalizedResultReportError::InvalidExport(_)
    ));
    assert_eq!(
        error.to_string(),
        "localized result report input is invalid"
    );
    assert!(error.source().is_some());
}

#[test]
fn zero_render_time_is_reported_as_invalid_export() {
    let snapshot = result_snapshot();
    let error = LocalizedResultReport::from_snapshot(
        &snapshot,
        LocalizedResultReportInput {
            report_ref: "localized_report_zero_time_v1",
            locale: "ko-KR",
            rendered_at_unix_ms: 0,
            limitations: &["검토된 제한사항입니다."],
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        LocalizedResultReportError::InvalidExport(_)
    ));
    assert_eq!(
        error.to_string(),
        "localized result report input is invalid"
    );
    assert!(error.source().is_some());
}
