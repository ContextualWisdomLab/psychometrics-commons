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
        Err(IdempotencyError::MalformedHeader) => {
            return bad_request("result export request contains a malformed HTTP header field");
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
        Err(AcceptError::MalformedHeader) => {
            bad_request("result export request contains a malformed HTTP header field")
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
    MalformedHeader,
    Invalid,
}

fn idempotency_key(request: &str) -> Result<&str, IdempotencyError> {
    let value = match single_header(request, "idempotency-key") {
        Ok(value) => value.ok_or(IdempotencyError::Missing)?,
        Err(HeaderError::Duplicate) => return Err(IdempotencyError::Duplicate),
        Err(HeaderError::Malformed) => return Err(IdempotencyError::MalformedHeader),
    };
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
    MalformedHeader,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcceptTarget {
    Json,
    Text,
    Any,
}

impl AcceptTarget {
    fn matches(self, representation: Representation) -> bool {
        matches!(
            (self, representation),
            (Self::Json, Representation::Json)
                | (Self::Text, Representation::Text)
                | (Self::Any, _)
        )
    }
}

fn accept_representation(request: &str) -> Result<Representation, AcceptError> {
    let accept = combined_header(request, "accept").map_err(|_| AcceptError::MalformedHeader)?;
    let Some(accept) = accept else {
        return Ok(Representation::Json);
    };

    let json_quality = representation_quality(&accept, Representation::Json).unwrap_or(0);
    let text_quality = representation_quality(&accept, Representation::Text).unwrap_or(0);
    match (json_quality, text_quality) {
        (0, 0) => Err(AcceptError::Unsupported),
        (json, text) if json >= text => Ok(Representation::Json),
        _ => Ok(Representation::Text),
    }
}

fn representation_quality(accept: &str, representation: Representation) -> Option<u16> {
    let mut best: Option<(u8, u16)> = None;
    for item in accept.split(',') {
        let Some((target, quality, specificity)) = parse_accept_item(item) else {
            continue;
        };
        if !target.matches(representation) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_specificity, best_quality)| {
                specificity > *best_specificity
                    || (specificity == *best_specificity && quality > *best_quality)
            })
        {
            best = Some((specificity, quality));
        }
    }
    best.map(|(_, quality)| quality)
}

fn parse_accept_item(item: &str) -> Option<(AcceptTarget, u16, u8)> {
    let mut segments = item.split(';');
    let media_range = segments.next()?.trim();
    let (target, mut specificity) = if media_range.eq_ignore_ascii_case("application/json") {
        (AcceptTarget::Json, 2)
    } else if media_range.eq_ignore_ascii_case("text/plain") {
        (AcceptTarget::Text, 2)
    } else if media_range.eq_ignore_ascii_case("application/*") {
        (AcceptTarget::Json, 1)
    } else if media_range.eq_ignore_ascii_case("text/*") {
        (AcceptTarget::Text, 1)
    } else if media_range == "*/*" {
        (AcceptTarget::Any, 0)
    } else {
        return None;
    };

    let mut quality = 1000;
    let mut saw_quality = false;
    let mut saw_charset = false;
    for parameter in segments {
        let (name, value) = parameter.trim().split_once('=')?;
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("q") {
            if saw_quality {
                return None;
            }
            saw_quality = true;
            quality = parse_quality(value)?;
        } else if target == AcceptTarget::Text
            && specificity == 2
            && name.eq_ignore_ascii_case("charset")
            && value.eq_ignore_ascii_case("utf-8")
        {
            if saw_charset {
                return None;
            }
            saw_charset = true;
            if !saw_quality {
                specificity = 3;
            }
        } else {
            return None;
        }
    }

    Some((target, quality, specificity))
}

fn parse_quality(value: &str) -> Option<u16> {
    match value {
        "0" => return Some(0),
        "1" => return Some(1000),
        _ => {}
    }

    let (whole, fraction) = value.split_once('.')?;
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if whole == "1" {
        return fraction.bytes().all(|byte| byte == b'0').then_some(1000);
    }
    if whole != "0" {
        return None;
    }
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u16>().ok()?
    };
    let scale = 10_u16.pow(u32::try_from(3 - fraction.len()).ok()?);
    Some(fraction_value * scale)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeaderError {
    Malformed,
    Duplicate,
}

fn single_header<'a>(
    request: &'a str,
    requested_name: &str,
) -> Result<Option<&'a str>, HeaderError> {
    let mut found = None;
    for line in request.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(HeaderError::Malformed);
        };
        if name.is_empty() {
            return Err(HeaderError::Malformed);
        }
        if !name.eq_ignore_ascii_case(requested_name) {
            continue;
        }
        if found.is_some() {
            return Err(HeaderError::Duplicate);
        }
        found = Some(value.trim());
    }
    Ok(found)
}

fn combined_header(request: &str, requested_name: &str) -> Result<Option<String>, HeaderError> {
    let mut combined = String::new();
    let mut found = false;
    for line in request.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(HeaderError::Malformed);
        };
        if name.is_empty() {
            return Err(HeaderError::Malformed);
        }
        if !name.eq_ignore_ascii_case(requested_name) {
            continue;
        }
        if found {
            combined.push_str(", ");
        }
        combined.push_str(value.trim());
        found = true;
    }
    Ok(found.then_some(combined))
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
        accept_representation, combined_header, exact_opaque_reference, idempotency_key,
        json_string, parse_accept_item, parse_export_route, parse_quality, parse_request_line,
        representation_quality, single_header, AcceptError, AcceptTarget, HeaderError,
        IdempotencyError, Representation, RouteParse,
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
            idempotency_key(
                "POST / HTTP/1.1\r\nIdempotency-Key: export_alpha\r\nMalformedHeader\r\n\r\n"
            ),
            Err(IdempotencyError::MalformedHeader)
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
            Err(AcceptError::MalformedHeader)
        );
        assert_eq!(
            accept_representation(
                "POST / HTTP/1.1\r\nAccept: application/json\r\nAccept: text/plain\r\n\r\n"
            ),
            Ok(Representation::Json)
        );
        assert_eq!(
            single_header("POST / HTTP/1.1\r\nHost: example.test\r\n\r\n", "accept"),
            Ok(None)
        );
        assert_eq!(
            single_header("POST / HTTP/1.1\r\nBroken\r\n\r\n", "accept"),
            Err(HeaderError::Malformed)
        );
        assert_eq!(
            combined_header(
                "POST / HTTP/1.1\r\nAccept: application/json\r\nAccept: text/plain\r\n\r\n",
                "accept"
            ),
            Ok(Some("application/json, text/plain".to_owned()))
        );
    }

    #[test]
    fn accept_parser_covers_supported_ranges_parameters_and_quality_edges() {
        assert_eq!(
            parse_accept_item("application/json"),
            Some((AcceptTarget::Json, 1000, 2))
        );
        assert_eq!(
            parse_accept_item("APPLICATION/*;q=0.5"),
            Some((AcceptTarget::Json, 500, 1))
        );
        assert_eq!(
            parse_accept_item("text/plain; charset=UTF-8; q=0.75"),
            Some((AcceptTarget::Text, 750, 3))
        );
        assert_eq!(
            parse_accept_item("text/plain;q=0.4;charset=utf-8"),
            Some((AcceptTarget::Text, 400, 2))
        );
        assert_eq!(
            parse_accept_item("text/*;q=0.25"),
            Some((AcceptTarget::Text, 250, 1))
        );
        assert_eq!(
            parse_accept_item("*/*;q=0.1"),
            Some((AcceptTarget::Any, 100, 0))
        );
        assert_eq!(parse_accept_item("application/xml"), None);
        assert_eq!(parse_accept_item("application/json;profile=alpha"), None);
        assert_eq!(parse_accept_item("text/plain;charset=iso-8859-1"), None);
        assert_eq!(
            parse_accept_item("text/plain;charset=utf-8;charset=utf-8"),
            None
        );
        assert_eq!(parse_accept_item("text/plain;q=0.5;q=0.4"), None);
        assert_eq!(parse_accept_item("text/plain;broken"), None);
        assert_eq!(parse_accept_item("text/plain;q=bogus"), None);

        assert_eq!(parse_quality("0"), Some(0));
        assert_eq!(parse_quality("1"), Some(1000));
        assert_eq!(parse_quality("0."), Some(0));
        assert_eq!(parse_quality("1."), Some(1000));
        assert_eq!(parse_quality("0.7"), Some(700));
        assert_eq!(parse_quality("0.25"), Some(250));
        assert_eq!(parse_quality("0.125"), Some(125));
        assert_eq!(parse_quality("1.000"), Some(1000));
        assert_eq!(parse_quality("1.001"), None);
        assert_eq!(parse_quality("2.0"), None);
        assert_eq!(parse_quality("0.1234"), None);
        assert_eq!(parse_quality("0.x"), None);
        assert_eq!(parse_quality("bogus"), None);

        assert_eq!(
            representation_quality(
                "text/plain;q=0.1, text/*;q=0.9, */*;q=1",
                Representation::Text
            ),
            Some(100)
        );
        assert_eq!(
            representation_quality("application/json;q=0.1, */*;q=0.9", Representation::Text),
            Some(900)
        );
        assert_eq!(
            accept_representation(
                "POST / HTTP/1.1\r\nAccept: text/plain;q=0.5, application/json;q=0.5\r\n\r\n"
            ),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation(
                "POST / HTTP/1.1\r\nAccept: application/json;q=0, text/plain;q=0\r\n\r\n"
            ),
            Err(AcceptError::Unsupported)
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
