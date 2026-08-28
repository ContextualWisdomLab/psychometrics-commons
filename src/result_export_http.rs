//! As-built in-process HTTP transport for one authorized personal result export.
//!
//! The caller supplies product-owned participant, result, and export records that
//! were loaded by the hosted runtime. This adapter never trusts tenant, owner, or
//! result identity from the request body. It authorizes the stored records before
//! comparing route/idempotency identity to those records, then returns either the
//! exact machine-readable JSON export or the exact human-readable report. Errors
//! use RFC 9457 problem details and do not echo participant/result/export references.

use crate::authorization::AuthorizationContext;
use crate::participant::ParticipantRecord;
use crate::reference::normalized_reference;
use crate::result::ResultSnapshot;
use crate::result_export::ResultExport;
use crate::result_export_authorization::authorize_result_export_read;

/// Public prefix for immutable result resources.
pub const RESULT_COLLECTION_PATH: &str = "/v1/results";

/// HTTP response produced by one personal result-export request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultExportHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
    allow: Option<&'static str>,
}

impl ResultExportHttpResponse {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.to_owned(),
            allow: None,
        }
    }

    fn text(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/plain; charset=utf-8",
            body: body.to_owned(),
            allow: None,
        }
    }

    fn problem(status: u16, type_uri: &str, title: &str, detail: &str) -> Self {
        Self {
            status,
            content_type: "application/problem+json",
            body: format!(
                "{{\"type\":{},\"title\":{},\"status\":{status},\"detail\":{}}}",
                json_string(type_uri),
                json_string(title),
                json_string(detail)
            ),
            allow: None,
        }
    }

    fn method_not_allowed() -> Self {
        let mut response = Self::problem(
            405,
            "urn:psychometrics-commons:problem:method-not-allowed",
            "Method Not Allowed",
            "personal result export supports POST only",
        );
        response.allow = Some("POST");
        response
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

    /// Return the response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Return the value for an `Allow` response header when one is required.
    #[must_use]
    pub const fn allow(&self) -> Option<&'static str> {
        self.allow
    }
}

/// Translate one raw HTTP/1.1 request into an authorized personal result export.
///
/// The supplied records must be server-owned stored records, never material
/// reconstructed from caller-provided tenant or owner values. The request uses
/// `POST /v1/results/{result_ref}/exports`; `Idempotency-Key` must be the exact
/// opaque export identity. Authorization is evaluated before route/export binding
/// comparisons so an unauthorized caller cannot use those differences as an
/// existence oracle. Query parameters are undefined for this operation and fail
/// closed rather than being silently ignored.
#[must_use]
pub fn handle_result_export_http_request(
    request: &str,
    actor: &AuthorizationContext,
    participant: &ParticipantRecord,
    result: &ResultSnapshot,
    export: &ResultExport,
) -> ResultExportHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return bad_request(
            "result export request must include one HTTP method, target, and version",
        );
    };

    if target.contains('?') {
        return ResultExportHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:unsupported-query",
            "Unsupported Query",
            "personal result export does not define query parameters; request the exact export operation",
        );
    }

    let result_snapshot_ref = match parse_export_route(target) {
        RouteParse::Matched(result_snapshot_ref) => result_snapshot_ref,
        RouteParse::InvalidReference => {
            return bad_request("result route reference must be an exact opaque non-numeric value");
        }
        RouteParse::NotFound => return not_found(),
    };

    if method != "POST" {
        return ResultExportHttpResponse::method_not_allowed();
    }

    let idempotency_key = match idempotency_key(request) {
        Ok(value) => value,
        Err(IdempotencyError::Missing) => {
            return bad_request("POST result export requires an opaque Idempotency-Key header");
        }
        Err(IdempotencyError::Duplicate) => {
            return bad_request("send exactly one Idempotency-Key header for result export");
        }
        Err(IdempotencyError::Invalid) => {
            return bad_request(
                "result export Idempotency-Key must be an exact opaque non-numeric value",
            );
        }
    };

    if authorize_result_export_read(actor, participant, result, export).is_err() {
        return ResultExportHttpResponse::problem(
            403,
            "urn:psychometrics-commons:problem:result-export-forbidden",
            "Forbidden",
            "the authenticated caller is not authorized to receive this personal result export",
        );
    }

    if result_snapshot_ref != result.result_snapshot_ref() {
        return not_found();
    }
    if idempotency_key != export.export_ref() {
        return ResultExportHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key is already bound to different personal result-export evidence",
        );
    }

    match accept_representation(request) {
        Ok(Representation::Json) => ResultExportHttpResponse::json(export.json_document()),
        Ok(Representation::Text) => ResultExportHttpResponse::text(export.human_readable_report()),
        Err(AcceptError::Duplicate) => {
            bad_request("send at most one Accept header for result export")
        }
        Err(AcceptError::Unsupported) => ResultExportHttpResponse::problem(
            406,
            "urn:psychometrics-commons:problem:not-acceptable",
            "Not Acceptable",
            "request application/json, text/plain, or */* for a personal result export",
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteParse<'a> {
    Matched(&'a str),
    InvalidReference,
    NotFound,
}

fn parse_export_route(target: &str) -> RouteParse<'_> {
    let Some(rest) = target.strip_prefix(RESULT_COLLECTION_PATH) else {
        return RouteParse::NotFound;
    };
    let Some(rest) = rest.strip_prefix('/') else {
        return RouteParse::NotFound;
    };
    let Some(result_snapshot_ref) = rest.strip_suffix("/exports") else {
        return RouteParse::NotFound;
    };
    if result_snapshot_ref.is_empty() || result_snapshot_ref.contains('/') {
        return RouteParse::NotFound;
    }
    if !exact_opaque_reference(result_snapshot_ref) {
        return RouteParse::InvalidReference;
    }
    RouteParse::Matched(result_snapshot_ref)
}

fn exact_opaque_reference(value: &str) -> bool {
    !value.contains('%')
        && !value.contains('#')
        && normalized_reference(value).is_some_and(|normalized| normalized == value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdempotencyError {
    Missing,
    Duplicate,
    Invalid,
}

fn idempotency_key(request: &str) -> Result<&str, IdempotencyError> {
    let value = single_header(request, "idempotency-key")
        .map_err(|_| IdempotencyError::Duplicate)?
        .ok_or(IdempotencyError::Missing)?;
    if exact_opaque_reference(value) {
        Ok(value)
    } else {
        Err(IdempotencyError::Invalid)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Representation {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcceptError {
    Duplicate,
    Unsupported,
}

fn accept_representation(request: &str) -> Result<Representation, AcceptError> {
    let accept = single_header(request, "accept").map_err(|_| AcceptError::Duplicate)?;
    match accept {
        None | Some("*/*" | "application/json") => Ok(Representation::Json),
        Some("text/plain") => Ok(Representation::Text),
        Some(_) => Err(AcceptError::Unsupported),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DuplicateHeader;

fn single_header<'a>(
    request: &'a str,
    requested_name: &str,
) -> Result<Option<&'a str>, DuplicateHeader> {
    let mut found = None;
    for line in request.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case(requested_name) {
            continue;
        }
        if found.is_some() {
            return Err(DuplicateHeader);
        }
        found = Some(value.trim());
    }
    Ok(found)
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || version != "HTTP/1.1" {
        None
    } else {
        Some((method, target))
    }
}

fn bad_request(detail: &str) -> ResultExportHttpResponse {
    ResultExportHttpResponse::problem(
        400,
        "urn:psychometrics-commons:problem:bad-request",
        "Bad Request",
        detail,
    )
}

fn not_found() -> ResultExportHttpResponse {
    ResultExportHttpResponse::problem(
        404,
        "urn:psychometrics-commons:problem:not-found",
        "Not Found",
        "no personal result export is available at this route",
    )
}

fn json_string(value: &str) -> String {
    let mut encoded = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{0008}' => encoded.push_str("\\b"),
            '\u{000c}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            other if (other as u32) < 0x20 => append_control_escape(&mut encoded, other),
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

fn append_control_escape(target: &mut String, character: char) {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let code_point = character as usize;
    target.push_str("\\u00");
    target.push(HEX_DIGITS[(code_point >> 4) & 0x0f] as char);
    target.push(HEX_DIGITS[code_point & 0x0f] as char);
}

#[cfg(test)]
mod tests {
    use super::{
        accept_representation, exact_opaque_reference, idempotency_key, json_string,
        parse_export_route, parse_request_line, single_header, AcceptError, IdempotencyError,
        Representation, RouteParse,
    };

    #[test]
    fn private_http_parsers_cover_fail_closed_edges() {
        assert_eq!(parse_request_line(""), None);
        assert_eq!(parse_request_line("POST"), None);
        assert_eq!(parse_request_line("POST /x"), None);
        assert_eq!(parse_request_line("POST /x HTTP/2"), None);
        assert_eq!(parse_request_line("POST /x HTTP/1.1 extra"), None);
        assert_eq!(parse_request_line("POST /x HTTP/1.1"), Some(("POST", "/x")));

        assert_eq!(parse_export_route("/v1/other"), RouteParse::NotFound);
        assert_eq!(
            parse_export_route("/v1/results/result_alpha"),
            RouteParse::NotFound
        );
        assert_eq!(
            parse_export_route("/v1/results//exports"),
            RouteParse::NotFound
        );
        assert_eq!(
            parse_export_route("/v1/results/result_alpha/extra/exports"),
            RouteParse::NotFound
        );
        assert_eq!(
            parse_export_route("/v1/results/123/exports"),
            RouteParse::InvalidReference
        );
        assert_eq!(
            parse_export_route("/v1/results/result%2Falpha/exports"),
            RouteParse::InvalidReference
        );
        assert_eq!(
            parse_export_route("/v1/results/result#alpha/exports"),
            RouteParse::InvalidReference
        );
        assert_eq!(
            parse_export_route("/v1/results/result_alpha/exports"),
            RouteParse::Matched("result_alpha")
        );
        assert!(exact_opaque_reference("result_alpha"));
        assert!(!exact_opaque_reference(" result_alpha"));

        assert_eq!(
            idempotency_key("POST / HTTP/1.1\r\n\r\n"),
            Err(IdempotencyError::Missing)
        );
        assert_eq!(
            idempotency_key("POST / HTTP/1.1\r\nIdempotency-Key: 123\r\n\r\n"),
            Err(IdempotencyError::Invalid)
        );
        assert_eq!(
            idempotency_key("POST / HTTP/1.1\r\nIdempotency-Key: export_alpha\r\n\r\n"),
            Ok("export_alpha")
        );
        assert_eq!(
            idempotency_key(
                "POST / HTTP/1.1\r\nIdempotency-Key: export_alpha\r\nIdempotency-Key: export_alpha\r\n\r\n"
            ),
            Err(IdempotencyError::Duplicate)
        );

        assert_eq!(
            accept_representation("POST / HTTP/1.1\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("POST / HTTP/1.1\r\nAccept: */*\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("POST / HTTP/1.1\r\nACCEPT: application/json\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("POST / HTTP/1.1\r\nAccept: text/plain\r\n\r\n"),
            Ok(Representation::Text)
        );
        assert_eq!(
            accept_representation("POST / HTTP/1.1\r\nBroken\r\nAccept: application/xml\r\n\r\n"),
            Err(AcceptError::Unsupported)
        );
        assert_eq!(
            accept_representation(
                "POST / HTTP/1.1\r\nAccept: application/json\r\nAccept: text/plain\r\n\r\n"
            ),
            Err(AcceptError::Duplicate)
        );
        assert_eq!(
            single_header("POST / HTTP/1.1\r\nHost: example.test\r\n\r\n", "accept"),
            Ok(None)
        );
    }

    #[test]
    fn problem_json_escapes_control_and_quote_syntax() {
        assert_eq!(
            json_string("a\"b\\c\nd\re\tf\u{0008}g\u{000c}h\u{0000}"),
            "\"a\\\"b\\\\c\\nd\\re\\tf\\bg\\fh\\u0000\""
        );
    }
}
