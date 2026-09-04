//! Personal result export copies immutable snapshot scores into JSON and a
//! human-readable report.
//!
//! A purchaser who finished a Korean Big Five form must see the same
//! Extraversion estimate and standard error in both artifacts. The export
//! keeps the owner `participant_ref` so authorized personal work is not
//! paralyzed by blanket masking. It does not invent a type score. HTTP
//! `POST /v1/results/{result_ref}/exports` stays a later slice.

#[path = "common/mod.rs"]
mod common;

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::result_export::{
    ResultExport, ResultExportError, ResultExportInput,
};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

const EXTRAVERSION: f64 = 0.42;
const EXTRAVERSION_SE: f64 = 0.18;
const NEUROTICISM: f64 = -0.31;
const NEUROTICISM_SE: f64 = 0.21;

fn published_big_five_snapshot() -> ResultSnapshot {
    let snapshot = frozen_snapshot(
        "session_big_five_ko_v1",
        "response_snapshot_big_five_ko_v1",
        &[ResponseWrite {
            server_event_ref: "event_item_001",
            client_event_ref: "client_item_001",
            item_version_ref: "item_version_001",
            payload_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_big_five_ko_v1",
            response_snapshot_ref: "response_snapshot_big_five_ko_v1",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_big_five_ko_v1",
        &request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored(
                "construct_extraversion",
                EXTRAVERSION,
                Some(EXTRAVERSION_SE),
            )
            .unwrap(),
            ScoreObservation::scored("construct_neuroticism", NEUROTICISM, Some(NEUROTICISM_SE))
                .unwrap(),
            ScoreObservation::without_score(
                "construct_openness",
                ObservationDisposition::Abstained,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let scoring_session = common::scoring_session(
        request.session_ref(),
        "participant_anonymous_ko_001",
        request.instrument_version_ref(),
    );
    ResultSnapshot::new(
        &scoring_session,
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_big_five_ko_v1",
            participant_ref: "participant_anonymous_ko_001",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_service_v1"],
            created_at_unix_ms: 1_700_000_000_000,
            supersedes_ref: None,
        },
    )
    .unwrap()
}

fn export_input<'a>() -> ResultExportInput<'a> {
    ResultExportInput {
        export_ref: "result_export_big_five_ko_v1",
        locale: "ko-KR",
        exported_at_unix_ms: 1_700_000_100_000,
        limitations: &[
            "이 결과는 진단, 채용 적격, 또는 고정된 성격 유형이 아닙니다.",
            "Continuous scores remain the measurement source of truth.",
        ],
    }
}

#[test]
fn personal_export_repeats_true_big_five_scores_in_json_and_report() {
    let snapshot = published_big_five_snapshot();
    let extraversion_before = snapshot.score_observations()[0].score();
    let export = ResultExport::from_snapshot(&snapshot, export_input()).unwrap();
    let retained = export.clone();

    assert_eq!(retained, export);
    assert!(format!("{export:?}").contains("participant_anonymous_ko_001"));
    assert_eq!(export.export_ref(), "result_export_big_five_ko_v1");
    assert_eq!(
        export.result_snapshot_ref(),
        "result_snapshot_big_five_ko_v1"
    );
    assert_eq!(export.participant_ref(), "participant_anonymous_ko_001");
    assert_eq!(export.locale(), "ko-KR");
    assert_eq!(
        export.instrument_version_ref(),
        "instrument_version_big_five_ko_v1"
    );
    assert_eq!(export.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(export.score_observations().len(), 3);
    assert_eq!(
        export.score_observations()[0].construct_ref(),
        "construct_extraversion"
    );
    assert_eq!(export.score_observations()[0].score(), Some(EXTRAVERSION));
    assert_eq!(
        export.score_observations()[0].standard_error(),
        Some(EXTRAVERSION_SE)
    );
    assert_eq!(
        export.score_observations()[2].disposition(),
        ObservationDisposition::Abstained
    );
    assert_eq!(export.score_observations()[2].score(), None);

    let json = export.json_document();
    assert!(json.contains("\"participant_ref\":\"participant_anonymous_ko_001\""));
    assert!(json.contains("\"construct_ref\":\"construct_extraversion\""));
    assert!(json.contains(&format!("\"score\":{EXTRAVERSION}")));
    assert!(json.contains(&format!("\"standard_error\":{EXTRAVERSION_SE}")));
    assert!(json.contains("\"construct_ref\":\"construct_neuroticism\""));
    assert!(json.contains(&format!("\"score\":{NEUROTICISM}")));
    assert!(json.contains("\"disposition\":\"abstained\""));
    assert!(!json.contains("\"score\":null") || json.contains("construct_openness"));
    assert!(!json.contains("16-type"));
    assert!(!json.contains("MBTI"));

    let report = export.human_readable_report();
    assert!(report.contains("participant_anonymous_ko_001"));
    assert!(report.contains("construct_extraversion"));
    assert!(report.contains(&EXTRAVERSION.to_string()));
    assert!(report.contains(&EXTRAVERSION_SE.to_string()));
    assert!(report.contains("construct_neuroticism"));
    assert!(report.contains(&NEUROTICISM.to_string()));
    assert!(report.contains("abstained"));
    assert!(report.contains("이 결과는 진단, 채용 적격, 또는 고정된 성격 유형이 아닙니다."));
    assert!(report.contains("Continuous scores remain the measurement source of truth."));

    assert_eq!(
        snapshot.score_observations()[0].score(),
        extraversion_before
    );
    assert_eq!(
        snapshot.result_snapshot_ref(),
        "result_snapshot_big_five_ko_v1"
    );
}

#[test]
fn personal_export_rejects_blank_identity_locale_time_and_limitations() {
    let snapshot = published_big_five_snapshot();

    let mut input = export_input();
    input.export_ref = " ";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidReference
    );

    input = export_input();
    input.export_ref = " result_export_big_five_ko_v1 ";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidReference
    );

    input = export_input();
    input.export_ref = "result_export_\u{0001}_big_five_ko_v1";
    let control_ref_error = ResultExport::from_snapshot(&snapshot, input).unwrap_err();
    assert_eq!(control_ref_error, ResultExportError::InvalidReference);
    assert!(control_ref_error.to_string().contains("opaque non-numeric"));
    assert!(std::error::Error::source(&control_ref_error).is_none());

    input = export_input();
    input.locale = "ko KR";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidLocale
    );

    input = export_input();
    input.locale = " ko-KR";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidLocale
    );

    input = export_input();
    input.locale = "ko-KR ";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidLocale
    );

    input = export_input();
    input.locale = "k";
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidLocale
    );

    input = export_input();
    input.limitations = &["Do not\ndiagnose from this export."];
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidText
    );

    input = export_input();
    input.exported_at_unix_ms = 0;
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidTimestamp
    );

    input = export_input();
    input.limitations = &[];
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::MissingLimitations
    );

    input = export_input();
    input.limitations = &[" "];
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidText
    );

    input = export_input();
    input.limitations = &[" Do not diagnose from this export. "];
    assert_eq!(
        ResultExport::from_snapshot(&snapshot, input).unwrap_err(),
        ResultExportError::InvalidText
    );
}

#[test]
fn personal_export_keeps_failed_score_absent_and_escapes_report_quotes() {
    let snapshot = frozen_snapshot(
        "session_big_five_en_v1",
        "response_snapshot_big_five_en_v1",
        &[ResponseWrite {
            server_event_ref: "event_item_002",
            client_event_ref: "client_item_002",
            item_version_ref: "item_version_002",
            payload_digest:
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_big_five_en_v1",
            response_snapshot_ref: "response_snapshot_big_five_en_v1",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_en_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_en_v1",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_big_five_en_v1",
        &request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored("construct_conscientiousness", 0.67, None).unwrap(),
            ScoreObservation::without_score(
                "construct_agreeableness",
                ObservationDisposition::Failed,
            )
            .unwrap(),
            ScoreObservation::without_score("construct_openness", ObservationDisposition::Excluded)
                .unwrap(),
        ],
    )
    .unwrap();
    let scoring_session = common::scoring_session(
        request.session_ref(),
        "participant_account_en_001",
        request.instrument_version_ref(),
    );
    let snapshot = ResultSnapshot::new(
        &scoring_session,
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_big_five_en_v1",
            participant_ref: "participant_account_en_001",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_service_v1", "consent_research_v1"],
            created_at_unix_ms: 1_700_000_200_000,
            supersedes_ref: None,
        },
    )
    .unwrap();

    let export = ResultExport::from_snapshot(
        &snapshot,
        ResultExportInput {
            export_ref: "result_export_big_five_en_v1",
            locale: "en-US",
            exported_at_unix_ms: 1_700_000_300_000,
            limitations: &["Do not treat this as \"employment fitness.\""],
        },
    )
    .unwrap();

    assert_eq!(export.scoring_version_ref(), "scoring_version_big_five_v1");
    assert_eq!(export.score_observations()[0].score(), Some(0.67));
    assert_eq!(export.score_observations()[0].standard_error(), None);
    assert_eq!(
        export.score_observations()[1].disposition(),
        ObservationDisposition::Failed
    );
    assert!(export.json_document().contains("\"norm_version_ref\":null"));
    assert!(export.json_document().contains("\"score\":0.67"));
    assert!(export.json_document().contains("\"standard_error\":null"));
    assert!(export
        .json_document()
        .contains("\"disposition\":\"failed\""));
    assert!(export
        .json_document()
        .contains("\"disposition\":\"excluded\""));
    assert!(export.human_readable_report().contains("excluded"));
    assert!(export
        .json_document()
        .contains("Do not treat this as \\\"employment fitness.\\\""));
    assert!(export.human_readable_report().contains("0.67"));
    assert!(export.human_readable_report().contains("failed"));
    assert!(export
        .human_readable_report()
        .contains("norm_version_ref: none"));
    assert!(export
        .human_readable_report()
        .contains("Do not treat this as \"employment fitness.\""));
}
