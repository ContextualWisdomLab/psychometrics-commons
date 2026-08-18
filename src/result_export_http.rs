//! As-built in-process HTTP transport for one authorized personal result export.
//!
//! The caller supplies product-owned participant, result, and export records that
//! were loaded by the hosted runtime. This adapter never trusts tenant, owner, or
//! result identity from the request body. It authorizes the stored records first,
//! then binds the opaque route identities to those records before returning either
//! the exact machine-readable JSON export or the exact human-readable report.
//! Errors use RFC 9457 problem details and do not echo participant/result/export
//! references.

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
            "personal result export supports GET only",
        );
        response.allow = Some("GET");
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
/// reconstructed from caller-provided tenant or owner values. Authorization is
/// evaluated before route-to-export binding so an unauthorized caller cannot use
/// binding errors as an existence oracle.
#[must_use]
pub fn handle_result_export_http_request(
    request: &str,
    actor: &AuthorizationContext,
    participant: &ParticipantRecord,
    result: &ResultSnapshot,
    export: &ResultExport,
) -> ResultExportHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return bad_request("result export request must include one HTTP method, target, and version");
    };

    let route = match parse_export_route(target) {
        RouteParse::Matched {
            result_snapshot_ref,
            export_ref,
        } => (result_snapshot_ref, export_ref),
        RouteParse::InvalidReference => {
            return bad_request("result and export route references must be exact opaque non-numeric values");
        }
        RouteParse::NotFound => return not_found(),
    };

    if method != "GET" {
        return ResultExportHttpResponse::method_not_allowed();
    }

    if authorize_result_export_read(actor, participant, result, export).is_err() {
        return ResultExportHttpResponse::problem(
            403,
            "urn:psychometrics-commons:problem:result-export-forbidden",
            "Forbidden",
            "the authenticated caller is not authorized to receive this personal result export",
        );
    }

    if route.0 != result.result_snapshot_ref() || route.1 != export.export_ref() {
        return not_found();
    }

    match accept_representation(request) {
        Ok(Representation::Json) => ResultExportHttpResponse::json(export.json_document()),
        Ok(Representation::Text) => ResultExportHttpResponse::text(export.human_readable_report()),
        Err(AcceptError::Duplicate) => bad_request("send at most one Accept header for result export"),
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
    Matched {
        result_snapshot_ref: &'a str,
        export_ref: &'a str,
    },
    InvalidReference,
    NotFound,
}

fn parse_export_route(target: &str) -> RouteParse<'_> {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let Some(rest) = path.strip_prefix("/v1/results/") else {
        return RouteParse::NotFound;
    };
    let Some((result_snapshot_ref, export_ref)) = rest.split_once("/exports/") else {
        return RouteParse::NotFound;
    };
    if result_snapshot_ref.is_empty() || export_ref.is_empty() || export_ref.contains('/') {
        return RouteParse::NotFound;
    }
    if !exact_opaque_reference(result_snapshot_ref) || !exact_opaque_reference(export_ref) {
        return RouteParse::InvalidReference;
    }
    RouteParse::Matched {
        result_snapshot_ref,
        export_ref,
    }
}

fn exact_opaque_reference(value: &str) -> bool {
    !value.contains('%') && normalized_reference(value) == Some(value)
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
    let mut accept = None;
    for line in request.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("accept") {
            continue;
        }
        if accept.is_some() {
            return Err(AcceptError::Duplicate);
        }
        accept = Some(value.trim());
    }
    match accept {
        None | Some("*/*") | Some("application/json") => Ok(Representation::Json),
        Some("text/plain") => Ok(Representation::Text),
        Some(_) => Err(AcceptError::Unsupported),
    }
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
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
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        accept_representation, exact_opaque_reference, json_string, parse_export_route,
        parse_request_line, AcceptError, Representation, RouteParse,
    };

    #[test]
    fn private_http_parsers_cover_fail_closed_edges() {
        assert_eq!(parse_request_line(""), None);
        assert_eq!(parse_request_line("GET"), None);
        assert_eq!(parse_request_line("GET /x"), None);
        assert_eq!(parse_request_line("GET /x HTTP/2"), None);
        assert_eq!(parse_request_line("GET /x HTTP/1.1 extra"), None);
        assert_eq!(parse_request_line("GET /x HTTP/1.1"), Some(("GET", "/x")));

        assert_eq!(parse_export_route("/v1/other"), RouteParse::NotFound);
        assert_eq!(parse_export_route("/v1/results/result_alpha"), RouteParse::NotFound);
        assert_eq!(parse_export_route("/v1/results//exports/export_alpha"), RouteParse::NotFound);
        assert_eq!(parse_export_route("/v1/results/result_alpha/exports/"), RouteParse::NotFound);
        assert_eq!(
            parse_export_route("/v1/results/result_alpha/exports/export_alpha/extra"),
            RouteParse::NotFound
        );
        assert_eq!(
            parse_export_route("/v1/results/123/exports/export_alpha"),
            RouteParse::InvalidReference
        );
        assert_eq!(
            parse_export_route("/v1/results/result_alpha/exports/export%2Falpha"),
            RouteParse::InvalidReference
        );
        assert_eq!(
            parse_export_route("/v1/results/result_alpha/exports/export_alpha?download=1"),
            RouteParse::Matched {
                result_snapshot_ref: "result_alpha",
                export_ref: "export_alpha",
            }
        );
        assert!(exact_opaque_reference("result_alpha"));
        assert!(!exact_opaque_reference(" result_alpha"));

        assert_eq!(
            accept_representation("GET / HTTP/1.1\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("GET / HTTP/1.1\r\nAccept: */*\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("GET / HTTP/1.1\r\nACCEPT: application/json\r\n\r\n"),
            Ok(Representation::Json)
        );
        assert_eq!(
            accept_representation("GET / HTTP/1.1\r\nAccept: text/plain\r\n\r\n"),
            Ok(Representation::Text)
        );
        assert_eq!(
            accept_representation("GET / HTTP/1.1\r\nBroken\r\nAccept: application/xml\r\n\r\n"),
            Err(AcceptError::Unsupported)
        );
        assert_eq!(
            accept_representation(
                "GET / HTTP/1.1\r\nAccept: application/json\r\nAccept: text/plain\r\n\r\n"
            ),
            Err(AcceptError::Duplicate)
        );
    }

    #[test]
    fn problem_json_escapes_reviewed_strings() {
        assert_eq!(json_string("a\"b\\c\nd\re\tf"), "\"a\\\"b\\\\c\\nd\\re\\tf\"");
    }
}
