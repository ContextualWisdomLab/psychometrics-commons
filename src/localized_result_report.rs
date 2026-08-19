//! Exact-locale participant-facing reports derived from immutable result exports.
//!
//! Psychometrics Commons already stores continuous scores and uncertainty in an
//! immutable result snapshot. This module changes only presentation labels: it
//! reuses [`crate::result_export::ResultExport`] for validation and score copying,
//! supports only the explicitly reviewed `ko-KR` and `en-US` locales, and fails
//! closed for every other locale. It never recomputes a score, uncertainty,
//! calibration value, norm, DIF result, or scientific gate.

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
    /// export. Only report labels and disposition words vary by locale.
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
        let labels = labels_for_locale(input.locale)
            .ok_or(LocalizedResultReportError::UnsupportedLocale)?;
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
        let text = render_report(&export, input.limitations, labels);

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
    limitations: &[&str],
    labels: ReportLabels,
) -> String {
    let mut report = String::new();
    report.push_str(labels.title);
    report.push('\n');
    report.push_str("report_ref: ");
    report.push_str(export.export_ref());
    report.push('\n');
    report.push_str("result_snapshot_ref: ");
    report.push_str(export.result_snapshot_ref());
    report.push('\n');
    report.push_str("participant_ref: ");
    report.push_str(export.participant_ref());
    report.push('\n');
    report.push_str("locale: ");
    report.push_str(export.locale());
    report.push('\n');
    report.push_str("instrument_version_ref: ");
    report.push_str(export.instrument_version_ref());
    report.push('\n');
    report.push_str("scoring_version_ref: ");
    report.push_str(export.scoring_version_ref());
    report.push('\n');
    report.push_str("engine_artifact_digest: ");
    report.push_str(export.engine_artifact_digest());
    report.push_str("\n\n");
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
