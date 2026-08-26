//! Public HTTP transport for recording response events on an active session.
//!
//! This slice exposes `POST /v1/sessions/{session_ref}/responses` over HTTP/1.1.
//! The server accepts one answer only while the injected session is Active and
//! the `item_version_ref` belongs to that session's published release. The
//! `Idempotency-Key` header is the client event reference. Sessions are not
//! created here; catalog list, session create, commands, and persistence remain
//! other families. Errors use RFC 9457 problem details and never echo raw
//! request bodies or payload bytes.

use crate::instrument::InstrumentRelease;
use crate::reference::normalized_reference;
use crate::response::{ResponseEvent, ResponseLedger, ResponseWrite, WriteError};
use crate::session::AssessmentSession;
use std::collections::HashMap;
use std::fmt::Write;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Bounded read/write timeout for one accepted response HTTP connection.
pub const RESPONSE_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted response HTTP request size, including headers and body.
pub const RESPONSE_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

/// In-process sessions, bound releases, and response ledgers.
pub struct ResponseHttpRuntime {
    sessions: HashMap<String, AssessmentSession>,
    releases: HashMap<String, InstrumentRelease>,
    ledgers: HashMap<String, ResponseLedger>,
    next_server_event_ref: String,
}

impl ResponseHttpRuntime {
    /// Create a runtime from injected sessions and their bound releases.
    ///
    /// `next_server_event_ref` is the opaque identity the next successful new
    /// response will mint. Unpublished or inactive sessions may be supplied for
    /// operator tests; the handler still rejects new answers on those rows.
    #[must_use]
    pub fn new(
        sessions: Vec<AssessmentSession>,
        releases: Vec<InstrumentRelease>,
        next_server_event_ref: impl Into<String>,
    ) -> Self {
        let sessions = sessions
            .into_iter()
            .map(|session| (session.session_ref().to_owned(), session))
            .collect();
        let releases = releases
            .into_iter()
            .map(|release| (release.manifest().release_ref().to_owned(), release))
            .collect();
        Self {
            sessions,
            releases,
            ledgers: HashMap::new(),
            next_server_event_ref: next_server_event_ref.into(),
        }
    }

    /// Replace the next minted server event reference after a successful write.
    pub fn replace_next_server_event_ref(&mut self, next_server_event_ref: impl Into<String>) {
        self.next_server_event_ref = next_server_event_ref.into();
    }

    /// Return how many accepted response events this process holds for a session.
    #[must_use]
    pub fn event_count(&self, session_ref: &str) -> usize {
        self.ledgers.get(session_ref).map_or(0, ResponseLedger::len)
    }
}

/// HTTP response produced by a public response-event request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl ResponseHttpResponse {
    fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
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

    /// Return the response body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Translate one raw HTTP/1.1 request into a response-event write.
///
/// Unknown methods, encoded or numeric session references, missing idempotency
/// keys, inactive sessions, items outside the bound release, and conflicting
/// replays fail closed with RFC 9457 problem details.
#[must_use]
pub fn handle_response_http_request(
    request: &str,
    runtime: &mut ResponseHttpRuntime,
) -> ResponseHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "response request must include an HTTP method and target",
        );
    };
    let path = split_target(target).0;
    match (method, response_collection_session_ref(path)) {
        ("POST", Some(session_ref)) => handle_post(request, session_ref, runtime),
        (_, Some(_)) => method_not_allowed(),
        _ => ResponseHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "response routes accept POST /v1/sessions/{session_ref}/responses only",
        ),
    }
}

/// Bind a blocking TCP listener for public response HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_response_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single response HTTP request.
///
/// The connection is closed after the response. Keep-alive, TLS, and other
/// public families are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_response_http(
    listener: &TcpListener,
    runtime: &mut ResponseHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(RESPONSE_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(RESPONSE_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_response_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn method_not_allowed() -> ResponseHttpResponse {
    ResponseHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "response routes accept POST /v1/sessions/{session_ref}/responses only",
    )
}

fn handle_post(
    request: &str,
    session_ref: &str,
    runtime: &mut ResponseHttpRuntime,
) -> ResponseHttpResponse {
    if !response_session_ref_is_transport_safe(session_ref) {
        return ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session_ref must be an exact opaque non-numeric session identity",
        );
    }
    let Some(idempotency_key) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/sessions/{session_ref}/responses requires an opaque Idempotency-Key header",
        );
    };
    let Some(body) = request_body(request) else {
        return ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "response write requires a JSON object body",
        );
    };
    let Some(write) = parse_write_body(body) else {
        return ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "response write requires item_version_ref and payload_digest strings only",
        );
    };
    record_response(runtime, session_ref, idempotency_key, &write)
}

fn record_response(
    runtime: &mut ResponseHttpRuntime,
    session_ref: &str,
    client_event_ref: &str,
    write: &ResponseWriteBody,
) -> ResponseHttpResponse {
    let Some(session) = runtime.sessions.get(session_ref) else {
        return ResponseHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:session-not-found",
            "Session Not Found",
            "Use GET /v1/sessions/{session_ref} to confirm the session exists in this process, then POST /v1/sessions/{session_ref}/responses",
        );
    };
    let release_ref = session.instrument_release_ref().to_owned();
    let Some(release) = runtime.releases.get(&release_ref) else {
        return ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:instrument-release-not-found",
            "Instrument Release Not Found",
            "response write requires the session's bound instrument release in this process catalog",
        );
    };
    if !release
        .manifest()
        .item_version_refs()
        .iter()
        .any(|item| item == &write.item_version_ref)
    {
        return ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:item-not-in-release",
            "Item Not In Release",
            "Use an item_version_ref from the session's published instrument release",
        );
    }
    let server_event_ref = runtime.next_server_event_ref.clone();
    let prior_len = runtime.event_count(session_ref);
    let recorded = {
        let ledger = runtime
            .ledgers
            .entry(session_ref.to_owned())
            .or_insert_with(|| {
                ResponseLedger::from_session(session)
                    .expect("stored session already passed product identity validation")
            });
        ledger.record(
            session,
            ResponseWrite {
                server_event_ref: &server_event_ref,
                client_event_ref,
                item_version_ref: &write.item_version_ref,
                payload_digest: &write.payload_digest,
            },
        )
    };
    match recorded {
        Ok(event) => {
            let created = runtime.event_count(session_ref) > prior_len;
            if created {
                runtime.next_server_event_ref =
                    format!("evt_response_{}", runtime.event_count(session_ref) + 1);
            }
            ResponseHttpResponse::json(
                if created { 201 } else { 200 },
                event_body(session_ref, &event),
            )
        }
        Err(error) => write_problem(error),
    }
}

fn write_problem(error: WriteError) -> ResponseHttpResponse {
    match error {
        WriteError::SessionNotActive(_) => ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:session-not-active",
            "Session Not Active",
            "Activate the session before posting responses",
        ),
        WriteError::InvalidReference => ResponseHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "response identity references must use exact opaque non-numeric spelling",
        ),
        WriteError::EmptyReference | WriteError::InvalidPayloadDigest => {
            ResponseHttpResponse::problem(
                400,
                "urn:psychometrics-commons:problem:invalid-payload-digest",
                "Invalid Payload Digest",
                "payload_digest must be canonical lowercase sha256 evidence",
            )
        }
        WriteError::IdempotencyConflict => ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different item_version_ref or payload_digest",
        ),
        WriteError::ServerReferenceConflict => ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:server-reference-conflict",
            "Server Reference Conflict",
            "server event reference was already used by another response event",
        ),
        WriteError::SnapshotRequiresCompleted(_) => ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:snapshot-requires-completed",
            "Snapshot Requires Completed",
            "response snapshots are created after POST /v1/sessions/{session_ref}/commands Complete",
        ),
        WriteError::SessionMismatch => ResponseHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:session-response-binding-mismatch",
            "Session Response Binding Mismatch",
            "the response ledger is not bound to the requested assessment session",
        ),
    }
}

struct ResponseWriteBody {
    item_version_ref: String,
    payload_digest: String,
}

fn parse_write_body(body: &str) -> Option<ResponseWriteBody> {
    let fields = parse_string_object(body)?;
    if fields.len() != 2 {
        return None;
    }
    Some(ResponseWriteBody {
        item_version_ref: fields.get("item_version_ref")?.clone(),
        payload_digest: fields.get("payload_digest")?.clone(),
    })
}

fn parse_string_object(body: &str) -> Option<HashMap<String, String>> {
    let trimmed = body.trim();
    let inner = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    let mut fields = HashMap::new();
    let mut rest = inner.trim();
    if rest.is_empty() {
        return Some(fields);
    }
    loop {
        let (key, after_key) = parse_json_string(rest)?;
        let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
        let (value, after_value) = parse_json_string(after_colon)?;
        if fields.insert(key, value).is_some() {
            return None;
        }
        let after_value = after_value.trim_start();
        if after_value.is_empty() {
            return Some(fields);
        }
        rest = after_value.strip_prefix(',')?.trim_start();
        if rest.is_empty() {
            return None;
        }
    }
}

fn parse_json_string(input: &str) -> Option<(String, &str)> {
    let rest = input.strip_prefix('"')?;
    let mut decoded = String::new();
    let mut chars = rest.char_indices();
    while let Some((index, character)) = chars.next() {
        match character {
            '"' => return Some((decoded, &rest[index + 1..])),
            '\\' => match chars.next()?.1 {
                '"' => decoded.push('"'),
                '\\' => decoded.push('\\'),
                'n' => decoded.push('\n'),
                'r' => decoded.push('\r'),
                't' => decoded.push('\t'),
                _ => return None,
            },
            character if character.is_control() => return None,
            character => decoded.push(character),
        }
    }
    None
}

fn event_body(session_ref: &str, event: &ResponseEvent) -> String {
    format!(
        "{{\"session_ref\":{},\"server_event_ref\":{},\"client_event_ref\":{},\"item_version_ref\":{},\"payload_digest\":{},\"sequence\":{}}}",
        json_string(session_ref),
        json_string(event.server_event_ref()),
        json_string(event.client_event_ref()),
        json_string(event.item_version_ref()),
        json_string(event.payload_digest()),
        event.sequence()
    )
}

fn response_collection_session_ref(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v1/sessions/")?;
    let (session_ref, tail) = rest.split_once('/')?;
    (tail == "responses" && !session_ref.is_empty()).then_some(session_ref)
}

/// Return whether a path-extracted session reference survives transport hygiene.
///
/// Percent escapes and whitespace cannot survive a well-formed request target,
/// yet the guard stays explicit so encoded or padded identities keep failing
/// closed even if target parsing ever relaxes. Numeric-like identities are
/// rejected by [`normalized_reference`].
fn response_session_ref_is_transport_safe(session_ref: &str) -> bool {
    !session_ref.contains('%')
        && !session_ref.chars().any(char::is_whitespace)
        && normalized_reference(session_ref) == Some(session_ref)
}

fn valid_idempotency_key(value: &str) -> Option<&str> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.chars().any(char::is_whitespace) {
        return None;
    }
    let numeric_like = normalized.chars().any(char::is_numeric)
        && normalized
            .chars()
            .all(|character| character.is_numeric() || matches!(character, '+' | '-' | '.' | ','));
    if numeric_like {
        None
    } else {
        Some(normalized)
    }
}

fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        if line.is_empty() {
            return None;
        }
        let (header_name, value) = line.split_once(':')?;
        header_name.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn request_body(request: &str) -> Option<&str> {
    let (headers, body) = request.split_once("\r\n\r\n")?;
    let content_length = header_value(headers, "content-length")?
        .parse::<usize>()
        .ok()?;
    if body.len() < content_length {
        return None;
    }
    Some(&body[..content_length])
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
    if buffer.len() > RESPONSE_HTTP_MAX_REQUEST_BYTES
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
                || buffer.len() > RESPONSE_HTTP_MAX_REQUEST_BYTES
            {
                Ok(RequestReadProgress::Complete)
            } else {
                Ok(RequestReadProgress::Continue)
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(RequestReadProgress::Complete)
        }
        Err(error) => Err(error),
    }
}

fn write_http_response(stream: &mut TcpStream, response: &ResponseHttpResponse) -> io::Result<()> {
    let body = response.body().as_bytes();
    let allow = if response.status() == 405 {
        "Allow: POST\r\n"
    } else {
        ""
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{allow}Connection: close\r\n\r\n",
        response.status(),
        reason_phrase(response.status()),
        response.content_type(),
        body.len()
    );
    io::Write::write_all(stream, header.as_bytes())?;
    io::Write::write_all(stream, body)
}

const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Error",
    }
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
    target.split_once('?').unwrap_or((target, ""))
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

#[cfg(test)]
mod unit_tests {
    use super::{
        apply_request_read, json_string, read_http_request, reason_phrase,
        response_collection_session_ref, response_session_ref_is_transport_safe, split_target,
        valid_idempotency_key, write_problem, RequestReadProgress, ResponseHttpRuntime,
        RESPONSE_HTTP_MAX_REQUEST_BYTES,
    };
    use crate::response::WriteError;
    use crate::session::SessionState;
    use std::io::{self, ErrorKind, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream};
    use std::time::Duration;

    #[test]
    fn helpers_cover_paths_status_escapes_and_read_progress() {
        assert_eq!(
            response_collection_session_ref("/v1/sessions/ses_one/responses"),
            Some("ses_one")
        );
        assert_eq!(
            response_collection_session_ref("/v1/sessions/ses_one/commands"),
            None
        );
        assert_eq!(response_collection_session_ref("/v1/sessions"), None);
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(
            split_target("/v1/sessions/ses_one/responses?x=1"),
            ("/v1/sessions/ses_one/responses", "x=1")
        );
        assert_eq!(valid_idempotency_key("idem_ok"), Some("idem_ok"));
        assert_eq!(valid_idempotency_key("42"), None);
        assert_eq!(valid_idempotency_key("   "), None);
        assert_eq!(valid_idempotency_key("idem spaced key"), None);
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(&mut Vec::new(), b"POST", Ok(4)).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; RESPONSE_HTTP_MAX_REQUEST_BYTES];
        assert!(matches!(
            apply_request_read(&mut oversized, b"y", Ok(1)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(
                &mut Vec::new(),
                b"",
                Err(io::Error::new(ErrorKind::TimedOut, "timeout"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(
                &mut Vec::new(),
                b"",
                Err(io::Error::new(ErrorKind::WouldBlock, "block"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(apply_request_read(&mut Vec::new(), b"", Err(io::Error::other("boom"))).is_err());
    }

    #[test]
    fn session_ref_guard_rejects_escapes_whitespace_aliases_and_numeric_identity() {
        assert!(response_session_ref_is_transport_safe(
            "ses_opaque_identity"
        ));
        assert!(!response_session_ref_is_transport_safe("%20ses_padded"));
        assert!(!response_session_ref_is_transport_safe("ses\u{00a0}padded"));
        assert!(!response_session_ref_is_transport_safe("42"));
    }

    #[test]
    fn write_problem_maps_every_error_to_its_stable_problem() {
        let cases = [
            (
                WriteError::SessionNotActive(SessionState::Scoring),
                409,
                "urn:psychometrics-commons:problem:session-not-active",
                "Session Not Active",
                "Activate the session before posting responses",
            ),
            (
                WriteError::InvalidReference,
                400,
                "urn:psychometrics-commons:problem:invalid-reference",
                "Invalid Reference",
                "response identity references must use exact opaque non-numeric spelling",
            ),
            (
                WriteError::EmptyReference,
                400,
                "urn:psychometrics-commons:problem:invalid-payload-digest",
                "Invalid Payload Digest",
                "payload_digest must be canonical lowercase sha256 evidence",
            ),
            (
                WriteError::InvalidPayloadDigest,
                400,
                "urn:psychometrics-commons:problem:invalid-payload-digest",
                "Invalid Payload Digest",
                "payload_digest must be canonical lowercase sha256 evidence",
            ),
            (
                WriteError::IdempotencyConflict,
                409,
                "urn:psychometrics-commons:problem:idempotency-conflict",
                "Idempotency Conflict",
                "Idempotency-Key was reused with a different item_version_ref or payload_digest",
            ),
            (
                WriteError::ServerReferenceConflict,
                409,
                "urn:psychometrics-commons:problem:server-reference-conflict",
                "Server Reference Conflict",
                "server event reference was already used by another response event",
            ),
            (
                WriteError::SnapshotRequiresCompleted(SessionState::Scoring),
                409,
                "urn:psychometrics-commons:problem:snapshot-requires-completed",
                "Snapshot Requires Completed",
                "response snapshots are created after POST /v1/sessions/{session_ref}/commands Complete",
            ),
            (
                WriteError::SessionMismatch,
                409,
                "urn:psychometrics-commons:problem:session-response-binding-mismatch",
                "Session Response Binding Mismatch",
                "the response ledger is not bound to the requested assessment session",
            ),
        ];
        for (error, status, type_uri, title, detail) in cases {
            let response = write_problem(error);
            assert_eq!(response.status(), status, "{type_uri}");
            assert_eq!(response.content_type(), "application/problem+json");
            assert!(response.body().contains(type_uri), "{type_uri}");
            assert!(response.body().contains(title), "{type_uri}");
            assert!(response.body().contains(detail), "{type_uri}");
        }
    }

    #[test]
    fn empty_runtime_reports_zero_events_and_accepts_cursor_rebinding() {
        let mut runtime = ResponseHttpRuntime::new(Vec::new(), Vec::new(), "evt_seed");
        assert_eq!(runtime.event_count("ses_unknown"), 0);
        runtime.replace_next_server_event_ref(String::from("evt_rebound"));
        assert_eq!(runtime.event_count("ses_unknown"), 0);
    }

    #[test]
    fn read_http_request_returns_a_complete_framed_request_from_the_socket() {
        let request = concat!(
            "POST /v1/sessions/ses_one/responses HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Idempotency-Key: idem_read_loop_happy_path\r\n",
            "Content-Length: 24\r\n",
            "\r\n",
            "{\"item_version_ref\":\"x\"}",
        );
        let (_client, mut server) = connected_stream(request.as_bytes());
        assert_eq!(read_http_request(&mut server).unwrap(), request);
    }

    fn connected_stream(payload: &[u8]) -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(payload).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        (client, server)
    }

    #[test]
    fn oversized_unframed_input_collapses_to_an_empty_request() {
        let (_client, mut server) =
            connected_stream(&[b'A'; RESPONSE_HTTP_MAX_REQUEST_BYTES + 808]);
        assert_eq!(read_http_request(&mut server).unwrap(), "");
    }

    #[test]
    fn input_without_header_terminator_collapses_to_an_empty_request() {
        let (_client, mut server) = connected_stream(
            b"POST /v1/sessions/ses_one/responses HTTP/1.1\r\nHost: localhost\r\n",
        );
        assert_eq!(read_http_request(&mut server).unwrap(), "");
    }
}
