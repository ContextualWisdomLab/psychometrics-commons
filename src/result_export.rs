//! Personal JSON and human-readable export of one immutable result snapshot.
//!
//! A purchaser who finished an assessment can copy the same continuous scores,
//! uncertainty, and version provenance into a personal archive. This module
//! does not recompute psychometric values or invent a Personality Style or type
//! score. The machine-readable JSON and typed accessors retain exact owner and
//! version provenance; the human-readable copy omits internal identifiers so a
//! participant does not need implementation terminology to understand the report.
//! HTTP transport remains a later slice.

use crate::reference::normalized_reference;
use crate::result::ResultSnapshot;
use crate::scoring::{ObservationDisposition, ScoreObservation};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Borrowed identity and approved limitation text for one personal result export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultExportInput<'a> {
    /// Opaque export identity used as the idempotency key for a later HTTP slice.
    pub export_ref: &'a str,
    /// Exact BCP 47-style locale of the participant-facing report text, such as `ko-KR`.
    pub locale: &'a str,
    /// Server-authoritative export time in milliseconds since the Unix epoch.
    ///
    /// The Unix epoch starts at 1970-01-01T00:00:00Z. A value of zero is invalid.
    pub exported_at_unix_ms: u64,
    /// Approved participant-facing limitations copied into both artifacts.
    pub limitations: &'a [&'a str],
}

/// Immutable personal export derived from one result snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultExport {
    export_ref: String,
    result_snapshot_ref: String,
    participant_ref: String,
    locale: String,
    exported_at_unix_ms: u64,
    instrument_version_ref: String,
    scoring_version_ref: String,
    engine_artifact_digest: String,
    score_observations: Vec<ScoreObservation>,
    json_document: String,
    human_readable_report: String,
}

/// Fail-closed validation error for personal result export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResultExportError {
    /// An opaque product reference was blank, padded, numeric-like, or unsafe.
    InvalidReference,
    /// The report locale is not an exact whitespace-free BCP 47-style tag.
    InvalidLocale,
    /// The export timestamp was zero or was before the result snapshot creation time.
    InvalidTimestamp,
    /// No participant-facing limitation text was supplied.
    MissingLimitations,
    /// Limitation text was blank, padded, or contained a control character.
    InvalidText,
}

impl Display for ResultExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "result export references must be opaque non-numeric values",
            Self::InvalidLocale => {
                "result export locale must be an exact whitespace-free BCP 47-style tag"
            }
            Self::InvalidTimestamp => {
                "result export timestamp must be nonzero and not precede result creation"
            }
            Self::MissingLimitations => {
                "personal result export must include participant-facing limitations"
            }
            Self::InvalidText => "result export text must be nonblank canonical display text",
        })
    }
}

impl Error for ResultExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl ResultExport {
    /// Copy one immutable snapshot into JSON and a human-readable personal report.
    ///
    /// Both artifacts repeat the stored construct scores and standard errors. An
    /// abstained, failed, or excluded observation keeps its disposition and does
    /// not receive an invented number. Exact owner/version provenance remains in
    /// the machine-readable document and typed fields while the human-readable
    /// report omits internal identifiers. The snapshot is not mutated. A report
    /// locale is a BCP 47-style tag such as `ko-KR`. Export timestamps are
    /// milliseconds since the Unix epoch (1970-01-01T00:00:00Z).
    ///
    /// # Errors
    ///
    /// Returns [`ResultExportError`] when the export identity, locale, timestamp,
    /// or limitation text is invalid. Export time can equal the result snapshot
    /// creation time. It must not be zero or before that creation time.
    pub fn from_snapshot(
        snapshot: &ResultSnapshot,
        input: ResultExportInput<'_>,
    ) -> Result<Self, ResultExportError> {
        let export_ref = required_reference(input.export_ref)?;
        if input.locale.trim() != input.locale || !valid_locale(input.locale) {
            return Err(ResultExportError::InvalidLocale);
        }
        if input.exported_at_unix_ms == 0
            || input.exported_at_unix_ms < snapshot.created_at_unix_ms()
        {
            return Err(ResultExportError::InvalidTimestamp);
        }
        if input.limitations.is_empty() {
            return Err(ResultExportError::MissingLimitations);
        }
        let mut limitations = Vec::with_capacity(input.limitations.len());
        for limitation in input.limitations {
            limitations.push(required_text(limitation)?);
        }

        let json_document = render_json_document(
            snapshot,
            export_ref,
            input.locale,
            input.exported_at_unix_ms,
            &limitations,
        );
        let human_readable_report = render_human_readable_report(snapshot, &limitations);

        Ok(Self {
            export_ref: export_ref.to_owned(),
            result_snapshot_ref: snapshot.result_snapshot_ref().to_owned(),
            participant_ref: snapshot.participant_ref().to_owned(),
            locale: input.locale.to_owned(),
            exported_at_unix_ms: input.exported_at_unix_ms,
            instrument_version_ref: snapshot.instrument_version_ref().to_owned(),
            scoring_version_ref: snapshot.scoring_version_ref().to_owned(),
            engine_artifact_digest: snapshot.engine_artifact_digest().to_owned(),
            score_observations: snapshot.score_observations().to_vec(),
            json_document,
            human_readable_report,
        })
    }

    /// Return the opaque export identity.
    #[must_use]
    pub fn export_ref(&self) -> &str {
        &self.export_ref
    }

    /// Return the immutable result snapshot that was exported.
    #[must_use]
    pub fn result_snapshot_ref(&self) -> &str {
        &self.result_snapshot_ref
    }

    /// Return the owner participant reference copied into the personal export.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the exact report locale.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the server-authoritative time recorded for this export.
    #[must_use]
    pub const fn exported_at_unix_ms(&self) -> u64 {
        self.exported_at_unix_ms
    }

    /// Return the published instrument version copied from the snapshot.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the scoring version copied from the snapshot.
    #[must_use]
    pub fn scoring_version_ref(&self) -> &str {
        &self.scoring_version_ref
    }

    /// Return the scoring-engine artifact digest copied from the snapshot.
    #[must_use]
    pub fn engine_artifact_digest(&self) -> &str {
        &self.engine_artifact_digest
    }

    /// Return copied construct-level observations without recomputation.
    #[must_use]
    pub fn score_observations(&self) -> &[ScoreObservation] {
        &self.score_observations
    }

    /// Return the machine-readable personal export document.
    #[must_use]
    pub fn json_document(&self) -> &str {
        &self.json_document
    }

    /// Return the human-readable personal report.
    #[must_use]
    pub fn human_readable_report(&self) -> &str {
        &self.human_readable_report
    }
}

fn render_json_document(
    snapshot: &ResultSnapshot,
    export_ref: &str,
    locale: &str,
    exported_at_unix_ms: u64,
    limitations: &[&str],
) -> String {
    let mut json = String::from("{");
    append_json_identity(&mut json, snapshot, export_ref);
    json.push(',');
    append_json_provenance(&mut json, snapshot, locale, exported_at_unix_ms);
    json.push(',');
    append_json_arrays(&mut json, snapshot, limitations);
    json.push('}');
    json
}

fn append_json_identity(json: &mut String, snapshot: &ResultSnapshot, export_ref: &str) {
    append_json_string(json, "export_ref", export_ref);
    json.push(',');
    append_json_string(json, "result_snapshot_ref", snapshot.result_snapshot_ref());
    json.push(',');
    append_json_string(json, "participant_ref", snapshot.participant_ref());
    json.push(',');
    append_json_string(json, "session_ref", snapshot.session_ref());
    json.push(',');
    append_json_string(
        json,
        "response_snapshot_ref",
        snapshot.response_snapshot_ref(),
    );
    json.push(',');
    append_json_string(json, "assessment_spec_ref", snapshot.assessment_spec_ref());
}

fn append_json_provenance(
    json: &mut String,
    snapshot: &ResultSnapshot,
    locale: &str,
    exported_at_unix_ms: u64,
) {
    append_json_string(
        json,
        "instrument_version_ref",
        snapshot.instrument_version_ref(),
    );
    json.push(',');
    append_json_string(json, "scoring_version_ref", snapshot.scoring_version_ref());
    json.push(',');
    append_json_string(
        json,
        "calibration_reference",
        snapshot.calibration_reference(),
    );
    json.push(',');
    json.push_str("\"norm_version_ref\":");
    match snapshot.norm_version_ref() {
        Some(norm_version_ref) => {
            json.push('"');
            append_escaped(json, norm_version_ref);
            json.push('"');
        }
        None => json.push_str("null"),
    }
    json.push(',');
    append_json_string(
        json,
        "narrative_version_ref",
        snapshot.narrative_version_ref(),
    );
    json.push(',');
    append_json_string(
        json,
        "engine_artifact_digest",
        snapshot.engine_artifact_digest(),
    );
    json.push(',');
    json.push_str("\"requested_output_schema_version\":");
    json.push_str(&snapshot.requested_output_schema_version().to_string());
    json.push(',');
    append_json_string(json, "locale", locale);
    json.push(',');
    json.push_str("\"created_at_unix_ms\":");
    json.push_str(&snapshot.created_at_unix_ms().to_string());
    json.push(',');
    json.push_str("\"exported_at_unix_ms\":");
    json.push_str(&exported_at_unix_ms.to_string());
}

fn append_json_arrays(json: &mut String, snapshot: &ResultSnapshot, limitations: &[&str]) {
    json.push_str("\"consent_snapshot_refs\":[");
    let consent_refs: Vec<&str> = snapshot
        .consent_snapshot_refs()
        .iter()
        .map(String::as_str)
        .collect();
    append_json_string_array(json, &consent_refs);
    json.push_str("],\"score_observations\":[");
    for (index, observation) in snapshot.score_observations().iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        append_json_observation(json, observation);
    }
    json.push_str("],\"limitations\":[");
    append_json_string_array(json, limitations);
    json.push(']');
}

fn append_json_string_array(json: &mut String, values: &[&str]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push('"');
        append_escaped(json, value);
        json.push('"');
    }
}

fn append_json_observation(json: &mut String, observation: &ScoreObservation) {
    json.push('{');
    append_json_string(json, "construct_ref", observation.construct_ref());
    json.push(',');
    append_json_string(
        json,
        "disposition",
        disposition_name(observation.disposition()),
    );
    json.push_str(",\"score\":");
    match observation.score() {
        Some(score) => json.push_str(&score.to_string()),
        None => json.push_str("null"),
    }
    json.push_str(",\"standard_error\":");
    match observation.standard_error() {
        Some(standard_error) => json.push_str(&standard_error.to_string()),
        None => json.push_str("null"),
    }
    json.push('}');
}

fn render_human_readable_report(snapshot: &ResultSnapshot, limitations: &[&str]) -> String {
    let mut report = String::from("Personal result export\n");
    report.push_str("Exact versions, time, ownership, and scoring evidence are retained in the machine-readable data export. Internal identifiers are omitted from this human-readable copy.\n\nScores\n");
    for observation in snapshot.score_observations() {
        report.push_str("- ");
        report.push_str(observation.construct_ref());
        report.push_str(": ");
        report.push_str(disposition_name(observation.disposition()));
        if let Some(score) = observation.score() {
            report.push(' ');
            report.push_str(&score.to_string());
            if let Some(standard_error) = observation.standard_error() {
                report.push_str(" (SE ");
                report.push_str(&standard_error.to_string());
                report.push(')');
            }
        }
        report.push('\n');
    }
    report.push_str("\nLimitations\n");
    for limitation in limitations {
        report.push_str("- ");
        report.push_str(limitation);
        report.push('\n');
    }
    report
}

fn append_json_string(json: &mut String, key: &str, value: &str) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":\"");
    append_escaped(json, value);
    json.push('"');
}

fn append_escaped(target: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => target.push_str("\\\""),
            '\\' => target.push_str("\\\\"),
            '\n' => target.push_str("\\n"),
            '\r' => target.push_str("\\r"),
            '\t' => target.push_str("\\t"),
            other => target.push(other),
        }
    }
}

const fn disposition_name(disposition: ObservationDisposition) -> &'static str {
    match disposition {
        ObservationDisposition::Scored => "scored",
        ObservationDisposition::Abstained => "abstained",
        ObservationDisposition::Failed => "failed",
        ObservationDisposition::Excluded => "excluded",
    }
}

fn required_reference(reference: &str) -> Result<&str, ResultExportError> {
    if reference.trim() != reference {
        return Err(ResultExportError::InvalidReference);
    }
    normalized_reference(reference).ok_or(ResultExportError::InvalidReference)
}

fn required_text(text: &str) -> Result<&str, ResultExportError> {
    if text.trim().is_empty() || text.trim() != text || text.chars().any(char::is_control) {
        Err(ResultExportError::InvalidText)
    } else {
        Ok(text)
    }
}

fn valid_locale(locale: &str) -> bool {
    locale.split('-').enumerate().all(|(index, subtag)| {
        if index == 0 {
            (2..=8).contains(&subtag.len()) && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
        } else {
            (1..=8).contains(&subtag.len())
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }
    })
}

#[cfg(test)]
mod export_guard_tests {
    use super::{
        append_escaped, append_json_string_array, disposition_name, required_reference,
        required_text, valid_locale, ResultExportError,
    };
    use crate::scoring::ObservationDisposition;

    #[test]
    fn guards_reject_numeric_locale_blank_text_and_name_every_disposition() {
        assert_eq!(
            required_reference("12"),
            Err(ResultExportError::InvalidReference)
        );
        assert_eq!(
            required_reference(" result_export_big_five_ko_v1 "),
            Err(ResultExportError::InvalidReference)
        );
        assert_eq!(
            required_reference("result_export_big_five_ko_v1").unwrap(),
            "result_export_big_five_ko_v1"
        );
        assert!(!valid_locale(""));
        assert!(!valid_locale("k"));
        assert!(!valid_locale("toolongprimary"));
        assert!(!valid_locale("k1"));
        assert!(!valid_locale("ko-"));
        assert!(!valid_locale("ko-THISISLONG"));
        assert!(!valid_locale("ko-K!"));
        assert!(valid_locale("ko"));
        assert!(valid_locale("ko-KR"));
        let mut encoded = String::new();
        append_json_string_array(&mut encoded, &[]);
        assert!(encoded.is_empty());
        append_json_string_array(&mut encoded, &["consent_service_v1", "consent_research_v1"]);
        assert_eq!(encoded, "\"consent_service_v1\",\"consent_research_v1\"");
        assert_eq!(
            required_text("\tlimitation"),
            Err(ResultExportError::InvalidText)
        );
        assert_eq!(
            required_text(" Do not diagnose from this export. "),
            Err(ResultExportError::InvalidText)
        );
        assert_eq!(
            required_text("Continuous scores remain the measurement source of truth.").unwrap(),
            "Continuous scores remain the measurement source of truth."
        );
        assert_eq!(disposition_name(ObservationDisposition::Failed), "failed");
        assert_eq!(
            disposition_name(ObservationDisposition::Excluded),
            "excluded"
        );
        let mut escaped = String::new();
        append_escaped(&mut escaped, "say \"no\"\\\n\r\t");
        assert_eq!(escaped, "say \\\"no\\\"\\\\\\n\\r\\t");
        for error in [
            ResultExportError::InvalidReference,
            ResultExportError::InvalidLocale,
            ResultExportError::InvalidTimestamp,
            ResultExportError::MissingLimitations,
            ResultExportError::InvalidText,
        ] {
            assert!(!error.to_string().is_empty());
            assert!(std::error::Error::source(&error).is_none());
        }
    }
}
