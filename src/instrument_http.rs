//! Public HTTP transport for listing startable published instrument releases.
//!
//! This slice exposes `GET /v1/instruments` and
//! `GET /v1/instruments/{instrument_ref}` over HTTP/1.1. Only releases whose
//! publication state currently accepts new sessions are visible. Draft,
//! suspended, and retired rows stay hidden so unpublished catalog work cannot
//! be discovered here. A purchaser uses the returned `release_ref` and `locale`
//! with `POST /v1/sessions`. Persistence and session HTTP remain other slices.
//! Errors use RFC 9457 problem details and never echo raw request bodies.

use crate::instrument::{InstrumentRelease, PublicationState};
use crate::reference::normalized_reference;
use std::fmt::Write;
use std::io::{self, Read, Write as IoWrite};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Public collection path for startable instrument releases.
pub const INSTRUMENT_COLLECTION_PATH: &str = "/v1/instruments";
/// Bounded read/write timeout for one accepted instrument HTTP connection.
pub const INSTRUMENT_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted instrument HTTP request size, including headers.
pub const INSTRUMENT_HTTP_MAX_REQUEST_BYTES: usize = 4_096;

/// In-process catalog of instrument releases the handler may disclose.
pub struct InstrumentHttpRuntime {
    releases: Vec<InstrumentRelease>,
}

impl InstrumentHttpRuntime {
    /// Create a runtime from the exact catalog the handler may inspect.
    ///
    /// Unpublished releases may be supplied for operator tests; the handler
    /// still hides them from public list and family responses.
    #[must_use]
    pub fn new(releases: Vec<InstrumentRelease>) -> Self {
        Self { releases }
    }

    /// Return how many catalog rows this process currently holds, including hidden ones.
    #[must_use]
    pub fn catalog_count(&self) -> usize {
        self.releases.len()
    }
}

/// HTTP response produced by a public instrument catalog request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentHttpResponse {
    status: u16,
    content_type: &'static str,
    allow: Option<&'static str>,
    body: String,
}

impl InstrumentHttpResponse {
    fn json(status: u16, body: String) -> Self {
        Self {
            status,
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

    /// Return the HTTP status code.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Return the response content type.
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// Return the RFC 9110 `Allow` field when the request method is rejected.
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

/// Translate one raw HTTP/1.1 request into a catalog list or family response.
///
/// Unknown methods, malformed/unsafe encoded references, numeric references,
/// and families with no startable published release fail closed with RFC 9457
/// problem details. Valid UTF-8 percent encoding is decoded exactly once so
/// visible multilingual opaque references remain addressable over HTTP.
#[must_use]
pub fn handle_instrument_http_request(
    request: &str,
    runtime: &InstrumentHttpRuntime,
) -> InstrumentHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return InstrumentHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "instrument request must include an HTTP method and target",
        );
    };
    let path = split_target(target).0;
    match (method, path) {
        ("GET", INSTRUMENT_COLLECTION_PATH) => handle_list(runtime),
        ("GET", path) => handle_family(path, runtime),
        (_, INSTRUMENT_COLLECTION_PATH) => method_not_allowed(),
        (_, path) if path.starts_with("/v1/instruments/") => method_not_allowed(),
        _ => InstrumentHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "instrument routes accept GET /v1/instruments and GET /v1/instruments/{instrument_ref} only",
        ),
    }
}

/// Bind a blocking TCP listener for public instrument HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_instrument_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single instrument HTTP request.
///
/// The connection is closed after the response. Keep-alive, TLS, and other
/// public families are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_instrument_http(
    listener: &TcpListener,
    runtime: &InstrumentHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(INSTRUMENT_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(INSTRUMENT_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_instrument_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn method_not_allowed() -> InstrumentHttpResponse {
    let mut response = InstrumentHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "instrument routes accept GET /v1/instruments and GET /v1/instruments/{instrument_ref} only",
    );
    response.allow = Some("GET");
    response
}

fn handle_list(runtime: &InstrumentHttpRuntime) -> InstrumentHttpResponse {
    InstrumentHttpResponse::json(200, releases_body(&published_releases(runtime)))
}

fn handle_family(path: &str, runtime: &InstrumentHttpRuntime) -> InstrumentHttpResponse {
    let Some(encoded_instrument_ref) = path
        .strip_prefix("/v1/instruments/")
        .filter(|value| !value.is_empty() && !value.contains('/'))
    else {
        return InstrumentHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "instrument routes accept GET /v1/instruments and GET /v1/instruments/{instrument_ref} only",
        );
    };
    let Some(instrument_ref) = decode_path_segment(encoded_instrument_ref) else {
        return invalid_instrument_reference();
    };
    if instrument_ref.contains('/')
        || instrument_ref.chars().any(char::is_whitespace)
        || normalized_reference(&instrument_ref) != Some(instrument_ref.as_str())
    {
        return invalid_instrument_reference();
    }
    let releases: Vec<&InstrumentRelease> = published_releases(runtime)
        .into_iter()
        .filter(|release| release.manifest().instrument_ref() == instrument_ref)
        .collect();
    if releases.is_empty() {
        return InstrumentHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:instrument-not-found",
            "Instrument Not Found",
            "Use GET /v1/instruments to list startable published releases, then POST /v1/sessions with a listed release_ref and locale",
        );
    }
    InstrumentHttpResponse::json(
        200,
        format!(
            "{{\"instrument_ref\":{},\"releases\":{}}}",
            json_string(&instrument_ref),
            release_array(&releases)
        ),
    )
}

fn invalid_instrument_reference() -> InstrumentHttpResponse {
    InstrumentHttpResponse::problem(
        400,
        "urn:psychometrics-commons:problem:bad-request",
        "Bad Request",
        "instrument_ref must be an opaque non-numeric family identity",
    )
}

fn decode_path_segment(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn published_releases(runtime: &InstrumentHttpRuntime) -> Vec<&InstrumentRelease> {
    let mut releases: Vec<&InstrumentRelease> = runtime
        .releases
        .iter()
        .filter(|release| release.state() == PublicationState::Published)
        .collect();
    releases.sort_by(|left, right| {
        let left_manifest = left.manifest();
        let right_manifest = right.manifest();
        left_manifest
            .instrument_ref()
            .cmp(right_manifest.instrument_ref())
            .then(left_manifest.locale().cmp(right_manifest.locale()))
            .then(
                left_manifest
                    .release_ref()
                    .cmp(right_manifest.release_ref()),
            )
    });
    releases
}

fn releases_body(releases: &[&InstrumentRelease]) -> String {
    format!("{{\"releases\":{}}}", release_array(releases))
}

fn release_array(releases: &[&InstrumentRelease]) -> String {
    let mut body = String::from("[");
    for (index, release) in releases.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&release_object(release));
    }
    body.push(']');
    body
}

fn release_object(release: &InstrumentRelease) -> String {
    let manifest = release.manifest();
    format!(
        "{{\"instrument_ref\":{},\"release_ref\":{},\"instrument_version_ref\":{},\"locale\":{},\"construct_ref\":{},\"content_digest\":{},\"intended_use_ref\":{},\"limitations_ref\":{},\"state\":\"published\"}}",
        json_string(manifest.instrument_ref()),
        json_string(manifest.release_ref()),
        json_string(manifest.instrument_version_ref()),
        json_string(manifest.locale()),
        json_string(manifest.construct_ref()),
        json_string(manifest.content_digest()),
        json_string(manifest.intended_use_ref()),
        json_string(manifest.limitations_ref())
    )
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if !version.starts_with("HTTP/") || parts.next().is_some() {
        return None;
    }
    Some((method, target))
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
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
            character if character.is_control() => {
                let _ = write!(encoded, "\\u{:04x}", u32::from(character));
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    }
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let read_result = stream.read(&mut chunk);
        match apply_request_read(&mut buffer, &chunk, read_result)? {
            RequestReadProgress::Continue => {}
            RequestReadProgress::Complete => break,
        }
    }
    if buffer.len() > INSTRUMENT_HTTP_MAX_REQUEST_BYTES
        || !buffer.windows(4).any(|window| window == b"\r\n\r\n")
    {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[derive(Debug)]
enum RequestReadProgress {
    Continue,
    Complete,
}

fn apply_request_read(
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    read_result: io::Result<usize>,
) -> io::Result<RequestReadProgress> {
    match read_result {
        Ok(0) => Ok(RequestReadProgress::Complete),
        Ok(read) => {
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n")
                || buffer.len() > INSTRUMENT_HTTP_MAX_REQUEST_BYTES
            {
                Ok(RequestReadProgress::Complete)
            } else {
                Ok(RequestReadProgress::Continue)
            }
        }
        Err(error)
            if error.kind() == io::ErrorKind::TimedOut
                || error.kind() == io::ErrorKind::WouldBlock =>
        {
            Ok(RequestReadProgress::Complete)
        }
        Err(error) => Err(error),
    }
}

fn write_http_response(
    stream: &mut TcpStream,
    response: &InstrumentHttpResponse,
) -> io::Result<()> {
    let allow_header = response
        .allow
        .map_or_else(String::new, |value| format!("Allow: {value}\r\n"));
    let payload = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason_phrase(response.status),
        response.content_type,
        allow_header,
        response.body.len(),
        response.body
    );
    stream.write_all(payload.as_bytes())
}

#[cfg(test)]
mod unit_tests {
    use super::{
        apply_request_read, json_string, parse_request_line, reason_phrase, split_target,
        RequestReadProgress, INSTRUMENT_HTTP_MAX_REQUEST_BYTES,
    };
    use std::io::{self, ErrorKind};

    #[test]
    fn helpers_cover_escapes_status_and_read_progress() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(
            split_target("/v1/instruments?x=1"),
            ("/v1/instruments", "x=1")
        );
        assert_eq!(split_target("/v1/instruments"), ("/v1/instruments", ""));
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(&mut Vec::new(), b"GET", Ok(3)).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; INSTRUMENT_HTTP_MAX_REQUEST_BYTES];
        assert!(matches!(
            apply_request_read(&mut oversized, b"y", Ok(1)).unwrap(),
            RequestReadProgress::Complete
        ));
        let timeout = apply_request_read(
            &mut Vec::new(),
            b"",
            Err(io::Error::new(ErrorKind::TimedOut, "timeout")),
        )
        .expect_err("read timeout must remain an I/O timeout");
        assert_eq!(timeout.kind(), ErrorKind::TimedOut);
        let would_block = apply_request_read(
            &mut Vec::new(),
            b"",
            Err(io::Error::new(ErrorKind::WouldBlock, "block")),
        )
        .expect_err("would-block must remain an I/O error");
        assert_eq!(would_block.kind(), ErrorKind::WouldBlock);
        assert!(apply_request_read(&mut Vec::new(), b"", Err(io::Error::other("boom"))).is_err());
        assert!(parse_request_line("GET /v1/instruments SMTP/1.0\r\n\r\n").is_none());
        assert!(parse_request_line("GET /v1/instruments HTTP/1.1 leftover\r\n\r\n").is_none());
    }
}
