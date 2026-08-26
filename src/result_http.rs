//! Public in-process HTTP boundary for reading one immutable personal result.
//!
//! `GET /v1/results/{result_ref}` returns the score observations and immutable
//! scoring provenance already stored in [`crate::result::ResultSnapshot`]. The
//! handler never recomputes psychometric values. Authorization is evaluated from
//! the server-owned participant and result records before the route reference is
//! compared with the stored result, so an unauthorized caller cannot use a
//! binding mismatch as an existence oracle.

use crate::authorization::AuthorizationContext;
use crate::participant::ParticipantRecord;
use crate::reference::normalized_reference;
use crate::result::ResultSnapshot;
use crate::result_authorization::authorize_result_read;
use crate::scoring::{ObservationDisposition, ScoreObservation};

/// Public path prefix for immutable result reads.
pub const RESULT_READ_PATH_PREFIX: &str = "/v1/results/";

/// HTTP response produced by one result-read request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultHttpResponse {
    status: u16,
    content_type: &'static str,
    allow: Option<&'static str>,
    body: String,
}

impl ResultHttpResponse {
    fn json(body: String) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            allow: None,
            body,
        }
    }

    fn problem(status: u16, type_uri: &str, title: &str, detail: &str) -> Self {
        Self {
            status,
            content_type: "application/problem+json",
            allow: None,
            body: format!(
                "{{\"type\":{},\"title\":{},\"status\":{status},\"detail\":{}}}",
                json_string(type_uri),
                json_string(title),
                json_string(detail)
            ),
        }
    }

    fn method_not_allowed(type_uri: &str, title: &str, detail: &str) -> Self {
        Self {
            status: 405,
            content_type: "application/problem+json",
            allow: Some("GET"),
            body: format!(
                "{{\"type\":{},\"title\":{},\"status\":405,\"detail\":{}}}",
                json_string(type_uri),
                json_string(title),
                json_string(detail)
            ),
        }
    }

    /// Return the HTTP status code.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Return the response media type.
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// Return the RFC 9110 `Allow` field value when this is a method rejection.
    #[must_use]
    pub const fn allow(&self) -> Option<&'static str> {
        self.allow
    }

    /// Return the response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Translate one HTTP/1.1 request into an authorized immutable result response.
///
/// `participant` and `result` must be server-owned records loaded for the
/// request. The actor never supplies resource ownership. Authorization is
/// intentionally checked before comparing the route reference with the stored
/// result identity. Query parameters are rejected until the repository defines
/// their semantics instead of being silently ignored.
#[must_use]
pub fn handle_result_http_request(
    request: &str,
    actor: &AuthorizationContext,
    participant: &ParticipantRecord,
    result: &ResultSnapshot,
) -> ResultHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return ResultHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "result read requires an HTTP/1.1 method and target",
        );
    };
    if target.contains('?') {
        return ResultHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:unsupported-query",
            "Unsupported Query",
            "result reads do not define query parameters; request the exact result resource",
        );
    }
    let Some(route_result_ref) = target
        .strip_prefix(RESULT_READ_PATH_PREFIX)
        .filter(|value| !value.is_empty() && !value.contains('/'))
    else {
        return ResultHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "result reads use GET /v1/results/{result_ref}",
        );
    };
    if method != "GET" {
        return ResultHttpResponse::method_not_allowed(
            "urn:psychometrics-commons:problem:method-not-allowed",
            "Method Not Allowed",
            "result reads use GET /v1/results/{result_ref}",
        );
    }
    if !canonical_route_reference(route_result_ref) {
        return ResultHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "use an exact opaque non-numeric result reference without URL-encoded aliases",
        );
    }
    if authorize_result_read(actor, participant, result).is_err() {
        return ResultHttpResponse::problem(
            403,
            "urn:psychometrics-commons:problem:result-access-denied",
            "Result Access Denied",
            "the authenticated context is not authorized to read this result",
        );
    }
    if route_result_ref != result.result_snapshot_ref() {
        return ResultHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:result-not-found",
            "Result Not Found",
            "no authorized immutable result matches the requested reference",
        );
    }

    ResultHttpResponse::json(result_body(result))
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if version != "HTTP/1.1" || parts.next().is_some() {
        return None;
    }
    Some((method, target))
}

fn canonical_route_reference(reference: &str) -> bool {
    !reference.contains('%')
        && !reference.contains('#')
        && normalized_reference(reference).is_some_and(|normalized| normalized == reference)
}

fn result_body(result: &ResultSnapshot) -> String {
    let mut json = String::from("{");
    append_json_string(&mut json, "result_ref", result.result_snapshot_ref());
    json.push(',');
    append_json_string(&mut json, "participant_ref", result.participant_ref());
    json.push(',');
    append_json_string(&mut json, "session_ref", result.session_ref());
    json.push(',');
    append_json_string(
        &mut json,
        "response_snapshot_ref",
        result.response_snapshot_ref(),
    );
    json.push(',');
    append_json_string(
        &mut json,
        "assessment_spec_ref",
        result.assessment_spec_ref(),
    );
    json.push(',');
    append_json_string(
        &mut json,
        "instrument_version_ref",
        result.instrument_version_ref(),
    );
    json.push(',');
    append_json_string(
        &mut json,
        "scoring_version_ref",
        result.scoring_version_ref(),
    );
    json.push(',');
    append_json_string(
        &mut json,
        "calibration_reference",
        result.calibration_reference(),
    );
    json.push_str(",\"norm_version_ref\":");
    match result.norm_version_ref() {
        Some(norm_version_ref) => {
            json.push('"');
            append_escaped(&mut json, norm_version_ref);
            json.push('"');
        }
        None => json.push_str("null"),
    }
    json.push(',');
    append_json_string(
        &mut json,
        "narrative_version_ref",
        result.narrative_version_ref(),
    );
    json.push_str(",\"consent_snapshot_refs\":[");
    for (index, consent_snapshot_ref) in result.consent_snapshot_refs().iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push('"');
        append_escaped(&mut json, consent_snapshot_ref);
        json.push('"');
    }
    json.push(']');
    json.push(',');
    append_json_string(
        &mut json,
        "engine_artifact_digest",
        result.engine_artifact_digest(),
    );
    json.push_str(",\"requested_output_schema_version\":");
    json.push_str(&result.requested_output_schema_version().to_string());
    json.push_str(",\"created_at_unix_ms\":");
    json.push_str(&result.created_at_unix_ms().to_string());
    json.push_str(",\"supersedes_ref\":");
    match result.supersedes_ref() {
        Some(supersedes_ref) => {
            json.push('"');
            append_escaped(&mut json, supersedes_ref);
            json.push('"');
        }
        None => json.push_str("null"),
    }
    json.push_str(",\"score_observations\":[");
    for (index, observation) in result.score_observations().iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        append_json_observation(&mut json, observation);
    }
    json.push_str("]}");
    json
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

fn append_json_string(json: &mut String, key: &str, value: &str) {
    json.push('"');
    append_escaped(json, key);
    json.push_str("\":\"");
    append_escaped(json, value);
    json.push('"');
}

fn json_string(value: &str) -> String {
    let mut json = String::from("\"");
    append_escaped(&mut json, value);
    json.push('"');
    json
}

fn append_escaped(target: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => target.push_str("\\\""),
            '\\' => target.push_str("\\\\"),
            '\u{0008}' => target.push_str("\\b"),
            '\u{000c}' => target.push_str("\\f"),
            '\n' => target.push_str("\\n"),
            '\r' => target.push_str("\\r"),
            '\t' => target.push_str("\\t"),
            other if (other as u32) < 0x20 => append_control_escape(target, other),
            other => target.push(other),
        }
    }
}

fn append_control_escape(target: &mut String, character: char) {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let code_point = character as usize;
    target.push_str("\\u00");
    target.push(HEX_DIGITS[(code_point >> 4) & 0x0f] as char);
    target.push(HEX_DIGITS[code_point & 0x0f] as char);
}

const fn disposition_name(disposition: ObservationDisposition) -> &'static str {
    match disposition {
        ObservationDisposition::Scored => "scored",
        ObservationDisposition::Abstained => "abstained",
        ObservationDisposition::Failed => "failed",
        ObservationDisposition::Excluded => "excluded",
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_route_reference, disposition_name, json_string, parse_request_line};
    use crate::scoring::ObservationDisposition;

    #[test]
    fn request_line_requires_exact_http_1_1_shape() {
        assert_eq!(
            parse_request_line("GET /v1/results/result_alpha HTTP/1.1\r\n\r\n"),
            Some(("GET", "/v1/results/result_alpha"))
        );
        assert_eq!(parse_request_line(""), None);
        assert_eq!(parse_request_line("GET"), None);
        assert_eq!(parse_request_line("GET /v1/results/result_alpha"), None);
        assert_eq!(
            parse_request_line("GET /v1/results/result_alpha HTTP/2\r\n\r\n"),
            None
        );
        assert_eq!(
            parse_request_line("GET /v1/results/result_alpha HTTP/1.1 extra\r\n\r\n"),
            None
        );
    }

    #[test]
    fn route_reference_rejects_aliases_and_numeric_identity() {
        assert!(canonical_route_reference("result_alpha"));
        assert!(!canonical_route_reference("12345"));
        assert!(!canonical_route_reference("result%5Falpha"));
        assert!(!canonical_route_reference("result#alpha"));
        assert!(!canonical_route_reference(" result_alpha "));
    }

    #[test]
    fn json_string_escapes_control_syntax() {
        assert_eq!(
            json_string("quote\" slash\\ newline\n return\r tab\t"),
            "\"quote\\\" slash\\\\ newline\\n return\\r tab\\t\""
        );
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(
            json_string("backspace\u{0008} form-feed\u{000c} nul\u{0000}"),
            "\"backspace\\b form-feed\\f nul\\u0000\""
        );
    }

    #[test]
    fn every_scoring_disposition_has_a_stable_wire_name() {
        assert_eq!(disposition_name(ObservationDisposition::Scored), "scored");
        assert_eq!(
            disposition_name(ObservationDisposition::Abstained),
            "abstained"
        );
        assert_eq!(disposition_name(ObservationDisposition::Failed), "failed");
        assert_eq!(
            disposition_name(ObservationDisposition::Excluded),
            "excluded"
        );
    }
}