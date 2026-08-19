//! Exact-locale participant-facing reports derived from immutable result exports.
//!
//! Psychometrics Commons already stores continuous scores and uncertainty in an
//! immutable result snapshot. A continuous score is a numeric estimate on a
//! measurement scale, while uncertainty describes how precise that estimate is.
//! Calibration links responses to the approved scoring model. A norm is a reviewed
//! comparison reference. Differential item functioning (DIF) checks whether an item
//! behaves differently across groups after accounting for the measured construct.
//! A scientific gate is a required evidence check that must pass before a result may
//! be published or compared.
//!
//! This module changes only presentation labels: it reuses
//! [`crate::result_export::ResultExport`] for validation and score copying, supports
//! only the explicitly reviewed `ko-KR` and `en-US` locales, and fails closed for
//! every other locale. It never recomputes any of those scientific values or gates.

use crate::result::ResultSnapshot;
use crate::result_export::{ResultExport, ResultExportError, ResultExportInput};
use crate::scoring::ObservationDisposition;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Borrowed inputs for one localized participant-facing result report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalizedResultReportInput<'a> {
    /// Opaque report identity; it is also used as the immutable export identity.
    pub report_ref: &'a str,
    /// Exact reviewed locale. This version supports only `ko-KR` and `en-US`.
    pub locale: &'a str,
    /// Server-authoritative render time as Unix milliseconds.
    pub rendered_at_unix_ms: u64,
    /// Reviewed participant-facing limitations copied without rewriting.
    pub limitations: &'a [&'a str],
}

/// Localized human-readable view over one immutable personal result export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalizedResultReport {
    report_ref: String,
    result_snapshot_ref: String,
    participant_ref: String,
    locale: String,
    text: String,
}

/// Fail-closed error while building a localized participant-facing report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalizedResultReportError {
    /// The requested locale does not have a reviewed label bundle in this runtime.
    UnsupportedLocale,
    /// The underlying immutable export rejected identity, time, or limitation input.
    InvalidExport(ResultExportError),
}

impl Display for LocalizedResultReportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedLocale => {
                "localized result reports require an exact supported locale: ko-KR or en-US"
            }
            Self::InvalidExport(_) => "localized result report input is invalid",
        })
    }
}

impl Error for LocalizedResultReportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::UnsupportedLocale => None,
            Self::InvalidExport(error) => Some(error),
        }
    }
}

impl LocalizedResultReport {
    /// Render one immutable result using a reviewed exact-locale label bundle.
    ///
    /// Numeric scores, standard errors, construct references, participant
    /// identity, and scientific provenance are copied from the existing result
    /// snapshot/export boundary. Only report labels and disposition words vary by
    /// locale.
    ///
    /// # Errors
    ///
    /// Returns [`LocalizedResultReportError::UnsupportedLocale`] for any locale
    /// other than exact `ko-KR` or `en-US`. Returns
    /// [`LocalizedResultReportError::InvalidExport`] when the underlying export
    /// rejects the report identity, render time, or limitation text.
    pub fn from_snapshot(
        snapshot: &ResultSnapshot,
        input: LocalizedResultReportInput<'_>,
    ) -> Result<Self, LocalizedResultReportError> {
        let labels =
            labels_for_locale(input.locale).ok_or(LocalizedResultReportError::UnsupportedLocale)?;
        let export = ResultExport::from_snapshot(
            snapshot,
            ResultExportInput {
                export_ref: input.report_ref,
                locale: input.locale,
                exported_at_unix_ms: input.rendered_at_unix_ms,
                limitations: input.limitations,
            },
        )
        .map_err(LocalizedResultReportError::InvalidExport)?;
        let text = render_report(
            &export,
            snapshot,
            input.rendered_at_unix_ms,
            input.limitations,
            labels,
        );

        Ok(Self {
            report_ref: export.export_ref().to_owned(),
            result_snapshot_ref: export.result_snapshot_ref().to_owned(),
            participant_ref: export.participant_ref().to_owned(),
            locale: export.locale().to_owned(),
            text,
        })
    }

    /// Return the opaque report identity.
    #[must_use]
    pub fn report_ref(&self) -> &str {
        &self.report_ref
    }

    /// Return the immutable result snapshot represented by this report.
    #[must_use]
    pub fn result_snapshot_ref(&self) -> &str {
        &self.result_snapshot_ref
    }

    /// Return the participant that owns this personal report.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the exact reviewed report locale.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the localized human-readable report text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Copy)]
struct ReportLabels {
    title: &'static str,
    report_ref: &'static str,
    result_snapshot_ref: &'static str,
    participant_ref: &'static str,
    locale: &'static str,
    scoring_result_ref: &'static str,
    session_ref: &'static str,
    response_snapshot_ref: &'static str,
    assessment_spec_ref: &'static str,
    instrument_version_ref: &'static str,
    scoring_version_ref: &'static str,
    calibration_reference: &'static str,
    norm_version_ref: &'static str,
    narrative_version_ref: &'static str,
    requested_output_schema_version: &'static str,
    consent_snapshot_refs: &'static str,
    engine_artifact_digest: &'static str,
    result_created_at_unix_ms: &'static str,
    report_rendered_at_unix_ms: &'static str,
    supersedes_ref: &'static str,
    none: &'static str,
    scores: &'static str,
    limitations: &'static str,
    scored: &'static str,
    abstained: &'static str,
    failed: &'static str,
    excluded: &'static str,
    standard_error: &'static str,
}

const EN_US: ReportLabels = ReportLabels {
    title: "Personal result report",
    report_ref: "report_ref",
    result_snapshot_ref: "result_snapshot_ref",
    participant_ref: "participant_ref",
    locale: "locale",
    scoring_result_ref: "scoring_result_ref",
    session_ref: "session_ref",
    response_snapshot_ref: "response_snapshot_ref",
    assessment_spec_ref: "assessment_spec_ref",
    instrument_version_ref: "instrument_version_ref",
    scoring_version_ref: "scoring_version_ref",
    calibration_reference: "calibration_reference",
    norm_version_ref: "norm_version_ref",
    narrative_version_ref: "narrative_version_ref",
    requested_output_schema_version: "requested_output_schema_version",
    consent_snapshot_refs: "consent_snapshot_refs",
    engine_artifact_digest: "engine_artifact_digest",
    result_created_at_unix_ms: "result_created_at_unix_ms",
    report_rendered_at_unix_ms: "report_rendered_at_unix_ms",
    supersedes_ref: "supersedes_ref",
    none: "none",
    scores: "Scores",
    limitations: "Limitations",
    scored: "scored",
    abstained: "abstained",
    failed: "failed",
    excluded: "excluded",
    standard_error: "SE",
};

const KO_KR: ReportLabels = ReportLabels {
    title: "개인 결과 보고서",
    report_ref: "보고서 참조값",
    result_snapshot_ref: "결과 스냅샷 참조값",
    participant_ref: "참가자 참조값",
    locale: "로케일",
    scoring_result_ref: "채점 결과 참조값",
    session_ref: "세션 참조값",
    response_snapshot_ref: "응답 스냅샷 참조값",
    assessment_spec_ref: "평가 명세 참조값",
    instrument_version_ref: "검사 버전 참조값",
    scoring_version_ref: "채점 버전 참조값",
    calibration_reference: "교정 참조값",
    norm_version_ref: "규준 버전 참조값",
    narrative_version_ref: "서술 버전 참조값",
    requested_output_schema_version: "출력 스키마 버전",
    consent_snapshot_refs: "동의 스냅샷 참조값",
    engine_artifact_digest: "채점 엔진 산출물 해시",
    result_created_at_unix_ms: "결과 생성 시각(Unix ms)",
    report_rendered_at_unix_ms: "보고서 생성 시각(Unix ms)",
    supersedes_ref: "대체 이전 결과 참조값",
    none: "없음",
    scores: "점수",
    limitations: "제한사항",
    scored: "채점됨",
    abstained: "보류",
    failed: "실패",
    excluded: "제외",
    standard_error: "표준오차",
};

fn labels_for_locale(locale: &str) -> Option<ReportLabels> {
    match locale {
        "en-US" => Some(EN_US),
        "ko-KR" => Some(KO_KR),
        _ => None,
    }
}

fn render_report(
    export: &ResultExport,
    snapshot: &ResultSnapshot,
    rendered_at_unix_ms: u64,
    limitations: &[&str],
    labels: ReportLabels,
) -> String {
    let mut report = String::new();
    report.push_str(labels.title);
    report.push('\n');
    append_metadata(&mut report, labels.report_ref, export.export_ref());
    append_metadata(
        &mut report,
        labels.result_snapshot_ref,
        export.result_snapshot_ref(),
    );
    append_metadata(&mut report, labels.participant_ref, export.participant_ref());
    append_metadata(&mut report, labels.locale, export.locale());
    append_metadata(
        &mut report,
        labels.scoring_result_ref,
        snapshot.scoring_result_ref(),
    );
    append_metadata(&mut report, labels.session_ref, snapshot.session_ref());
    append_metadata(
        &mut report,
        labels.response_snapshot_ref,
        snapshot.response_snapshot_ref(),
    );
    append_metadata(
        &mut report,
        labels.assessment_spec_ref,
        snapshot.assessment_spec_ref(),
    );
    append_metadata(
        &mut report,
        labels.instrument_version_ref,
        export.instrument_version_ref(),
    );
    append_metadata(
        &mut report,
        labels.scoring_version_ref,
        export.scoring_version_ref(),
    );
    append_metadata(
        &mut report,
        labels.calibration_reference,
        snapshot.calibration_reference(),
    );
    append_metadata(
        &mut report,
        labels.norm_version_ref,
        snapshot.norm_version_ref().unwrap_or(labels.none),
    );
    append_metadata(
        &mut report,
        labels.narrative_version_ref,
        snapshot.narrative_version_ref(),
    );
    append_metadata(
        &mut report,
        labels.requested_output_schema_version,
        &snapshot.requested_output_schema_version().to_string(),
    );
    append_metadata(
        &mut report,
        labels.consent_snapshot_refs,
        &snapshot.consent_snapshot_refs().join(", "),
    );
    append_metadata(
        &mut report,
        labels.engine_artifact_digest,
        export.engine_artifact_digest(),
    );
    append_metadata(
        &mut report,
        labels.result_created_at_unix_ms,
        &snapshot.created_at_unix_ms().to_string(),
    );
    append_metadata(
        &mut report,
        labels.report_rendered_at_unix_ms,
        &rendered_at_unix_ms.to_string(),
    );
    append_metadata(
        &mut report,
        labels.supersedes_ref,
        snapshot.supersedes_ref().unwrap_or(labels.none),
    );
    report.push('\n');
    report.push_str(labels.scores);
    report.push('\n');

    for observation in export.score_observations() {
        report.push_str("- ");
        report.push_str(observation.construct_ref());
        report.push_str(": ");
        report.push_str(disposition_label(observation.disposition(), labels));
        if let Some(score) = observation.score() {
            report.push(' ');
            report.push_str(&score.to_string());
            if let Some(standard_error) = observation.standard_error() {
                report.push_str(" (");
                report.push_str(labels.standard_error);
                report.push(' ');
                report.push_str(&standard_error.to_string());
                report.push(')');
            }
        }
        report.push('\n');
    }

    report.push('\n');
    report.push_str(labels.limitations);
    report.push('\n');
    for limitation in limitations {
        report.push_str("- ");
        report.push_str(limitation);
        report.push('\n');
    }
    report
}

fn append_metadata(report: &mut String, label: &str, value: &str) {
    report.push_str(label);
    report.push_str(": ");
    report.push_str(value);
    report.push('\n');
}

const fn disposition_label(
    disposition: ObservationDisposition,
    labels: ReportLabels,
) -> &'static str {
    match disposition {
        ObservationDisposition::Scored => labels.scored,
        ObservationDisposition::Abstained => labels.abstained,
        ObservationDisposition::Failed => labels.failed,
        ObservationDisposition::Excluded => labels.excluded,
    }
}

#[cfg(test)]
mod tests {
    use super::{disposition_label, labels_for_locale, EN_US, KO_KR};
    use crate::scoring::ObservationDisposition;

    #[test]
    fn exact_locale_resolution_and_disposition_labels_are_closed() {
        assert!(labels_for_locale("ko-KR").is_some());
        assert!(labels_for_locale("en-US").is_some());
        assert!(labels_for_locale("ko").is_none());
        assert!(labels_for_locale("en-us").is_none());
        assert_eq!(
            disposition_label(ObservationDisposition::Scored, EN_US),
            "scored"
        );
        assert_eq!(
            disposition_label(ObservationDisposition::Abstained, KO_KR),
            "보류"
        );
        assert_eq!(
            disposition_label(ObservationDisposition::Failed, KO_KR),
            "실패"
        );
        assert_eq!(
            disposition_label(ObservationDisposition::Excluded, KO_KR),
            "제외"
        );
    }
}
