//! Public HTTP transport for persist-backed assessment-session start and reload.
//!
//! This slice exposes `POST /v1/sessions` and `GET /v1/sessions/{session_ref}`
//! over HTTP/1.1. `Idempotency-Key` is the durable session reference. Create
//! calls [`start_created_assessment_session_from_stored_release`] so a stale
//! in-memory catalog cannot mint a session after persist Suspend or Retire.
//! Exact replay after that later persist returns the original session. Errors
//! use RFC 9457 problem details and never echo raw request bodies or SQL.

use crate::postgres_assessment_session::{
    load_assessment_session, start_created_assessment_session_from_stored_release,
    AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError,
    AssessmentSessionStartError,
};
use crate::reference::normalized_reference;
use crate::session::AssessmentSession;
use postgres::Transaction;
use std::collections::HashMap;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Public collection path for assessment sessions.
pub const SESSION_COLLECTION_PATH: &str = "/v1/sessions";
/// Bounded read/write timeout for one accepted session HTTP connection.
pub const SESSION_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted session HTTP request size, including headers and body.
pub const SESSION_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const VERSION: &str = "instrument_version_big_five_ko_v1";

/// Store used by session HTTP to start and reload created sessions.
pub trait SessionHttpPort {
    /// Start or replay one created session from stored publication evidence.
    ///
    /// # Errors
    ///
    /// Returns [`AssessmentSessionStartError`] when the catalog is unpublished
    /// and no exact stored start exists, or when identity conflicts.
    fn start_from_stored_release(
        &mut self,
        session_ref: &str,
        participant_ref: &str,
        instrument_release_ref: &str,
        locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<
        (AssessmentSession, AssessmentSessionPersistenceDisposition),
        AssessmentSessionStartError,
    >;

    /// Load one stored session after restart.
    ///
    /// # Errors
    ///
    /// Returns [`AssessmentSessionPersistenceError`] when the reference is
    /// invalid or stored identity cannot be restored.
    fn load(
        &mut self,
        session_ref: &str,
    ) -> Result<Option<AssessmentSession>, AssessmentSessionPersistenceError>;
}

/// In-memory port that preserves the sealed persist start contract for tests.
pub struct MemorySessionHttpPort {
    sessions: HashMap<String, AssessmentSession>,
    /// Whether the catalog currently accepts new sessions.
    pub published: bool,
    /// Injected start failure used to prove RFC 9457 mappings.
    pub next_start_error: Option<AssessmentSessionStartError>,
    /// Injected load failure used to prove RFC 9457 mappings.
    pub next_load_error: Option<AssessmentSessionPersistenceError>,
    /// Locale last passed to start, so tests can prove the persist path was used.
    pub last_start_locale: Option<String>,
}

impl MemorySessionHttpPort {
    /// Create a port whose catalog currently accepts new Korean Big Five starts.
    #[must_use]
    pub fn published() -> Self {
        Self {
            sessions: HashMap::new(),
            published: true,
            next_start_error: None,
            next_load_error: None,
            last_start_locale: None,
        }
    }
}

impl SessionHttpPort for MemorySessionHttpPort {
    fn start_from_stored_release(
        &mut self,
        session_ref: &str,
        participant_ref: &str,
        instrument_release_ref: &str,
        locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<
        (AssessmentSession, AssessmentSessionPersistenceDisposition),
        AssessmentSessionStartError,
    > {
        self.last_start_locale = Some(locale.to_owned());
        if let Some(error) = self.next_start_error.take() {
            return Err(error);
        }
        if let Some(stored) = self.sessions.get(session_ref) {
            if stored.participant_ref() == participant_ref
                && stored.instrument_release_ref() == instrument_release_ref
                && stored.locale() == locale
                && stored.created_at_unix_ms() == created_at_unix_ms
            {
                return Ok((
                    stored.clone(),
                    AssessmentSessionPersistenceDisposition::Duplicate,
                ));
            }
            return Err(AssessmentSessionStartError::Persistence(
                AssessmentSessionPersistenceError::ConflictingReplay,
            ));
        }
        if locale != "ko-KR" {
            return Err(AssessmentSessionStartError::LocaleMismatch);
        }
        if !self.published {
            return Err(AssessmentSessionStartError::InstrumentReleaseUnavailable);
        }
        let session = AssessmentSession::from_persisted_created(
            session_ref,
            participant_ref,
            instrument_release_ref,
            VERSION,
            DIGEST,
            locale,
            created_at_unix_ms,
        )
        .map_err(|_| AssessmentSessionStartError::InvalidReference)?;
        self.sessions
            .insert(session.session_ref().to_owned(), session.clone());
        Ok((session, AssessmentSessionPersistenceDisposition::Inserted))
    }

    fn load(
        &mut self,
        session_ref: &str,
    ) -> Result<Option<AssessmentSession>, AssessmentSessionPersistenceError> {
        if let Some(error) = self.next_load_error.take() {
            return Err(error);
        }
        if normalized_reference(session_ref).is_none() {
            return Err(AssessmentSessionPersistenceError::InvalidReference);
        }
        Ok(self.sessions.get(session_ref).cloned())
    }
}

/// `PostgreSQL` port that uses the sealed persist start and load functions.
pub struct PostgresSessionHttpPort<'a, 'b> {
    transaction: &'a mut Transaction<'b>,
}

impl<'a, 'b> PostgresSessionHttpPort<'a, 'b> {
    /// Borrow one caller-owned `READ COMMITTED` transaction.
    pub fn new(transaction: &'a mut Transaction<'b>) -> Self {
        Self { transaction }
    }
}

impl SessionHttpPort for PostgresSessionHttpPort<'_, '_> {
    fn start_from_stored_release(
        &mut self,
        session_ref: &str,
        participant_ref: &str,
        instrument_release_ref: &str,
        locale: &str,
        created_at_unix_ms: u64,
    ) -> Result<
        (AssessmentSession, AssessmentSessionPersistenceDisposition),
        AssessmentSessionStartError,
    > {
        start_created_assessment_session_from_stored_release(
            self.transaction,
            session_ref,
            participant_ref,
            instrument_release_ref,
            locale,
            created_at_unix_ms,
        )
    }

    fn load(
        &mut self,
        session_ref: &str,
    ) -> Result<Option<AssessmentSession>, AssessmentSessionPersistenceError> {
        load_assessment_session(self.transaction, session_ref)
    }
}

/// HTTP response produced by a public session request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl SessionHttpResponse {
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

/// Translate one raw HTTP/1.1 request into a persist-backed session response.
#[must_use]
pub fn handle_session_http_request<P: SessionHttpPort>(
    request: &str,
    port: &mut P,
    created_at_unix_ms: u64,
) -> SessionHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session request must include an HTTP method and target",
        );
    };
    let path = split_target(target).0;
    match (method, path) {
        ("POST", SESSION_COLLECTION_PATH) => handle_create(request, port, created_at_unix_ms),
        ("GET", path) => handle_get(path, port),
        (_, SESSION_COLLECTION_PATH) => method_not_allowed(),
        (_, path) if path.starts_with("/v1/sessions/") => method_not_allowed(),
        _ => SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
        ),
    }
}

/// Bind a blocking TCP listener for public session HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_session_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single persist-backed session request.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_session_http<P: SessionHttpPort>(
    listener: &TcpListener,
    port: &mut P,
    created_at_unix_ms: u64,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(SESSION_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(SESSION_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_session_http_request(&request, port, created_at_unix_ms);
    write_http_response(&mut stream, &response)
}

fn method_not_allowed() -> SessionHttpResponse {
    SessionHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
    )
}

fn handle_create<P: SessionHttpPort>(
    request: &str,
    port: &mut P,
    created_at_unix_ms: u64,
) -> SessionHttpResponse {
    let Some(session_ref) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/sessions requires an opaque Idempotency-Key header; reuse it to resume the same session",
        );
    };
    let Some(body) = request_body(request) else {
        return SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session create requires a JSON object body",
        );
    };
    let Some(create) = parse_create_body(body) else {
        return SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session create requires participant_ref, instrument_release_ref, and locale strings",
        );
    };
    match port.start_from_stored_release(
        session_ref,
        &create.participant_ref,
        &create.instrument_release_ref,
        &create.locale,
        created_at_unix_ms,
    ) {
        Ok((session, AssessmentSessionPersistenceDisposition::Inserted)) => {
            SessionHttpResponse::json(201, session_body(&session))
        }
        Ok((session, AssessmentSessionPersistenceDisposition::Duplicate)) => {
            SessionHttpResponse::json(200, session_body(&session))
        }
        Err(error) => start_problem(&error),
    }
}

fn handle_get<P: SessionHttpPort>(path: &str, port: &mut P) -> SessionHttpResponse {
    let Some(session_ref) = path
        .strip_prefix("/v1/sessions/")
        .filter(|value| !value.is_empty() && !value.contains('/'))
    else {
        if path == SESSION_COLLECTION_PATH {
            return method_not_allowed();
        }
        return SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
        );
    };
    match port.load(session_ref) {
        Ok(Some(session)) => SessionHttpResponse::json(200, session_body(&session)),
        Ok(None) => SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:session-not-found",
            "Session Not Found",
            "Use POST /v1/sessions with the same Idempotency-Key to start the session",
        ),
        Err(AssessmentSessionPersistenceError::InvalidReference) => SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "use an opaque non-numeric session reference to load a stored session",
        ),
        Err(_) => SessionHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:session-store-unavailable",
            "Session Store Unavailable",
            "retry GET /v1/sessions/{session_ref} after the session store is repaired",
        ),
    }
}

fn start_problem(error: &AssessmentSessionStartError) -> SessionHttpResponse {
    match error {
        AssessmentSessionStartError::InvalidReference => SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "use opaque non-numeric session and participant references to start a session",
        ),
        AssessmentSessionStartError::InvalidTimestamp => SessionHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:invalid-server-timestamp",
            "Invalid Server Timestamp",
            "session create requires a positive server clock",
        ),
        AssessmentSessionStartError::InstrumentReleaseUnavailable
        | AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::UnpublishedStart,
        ) => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:instrument-release-unavailable",
            "Instrument Release Unavailable",
            "publish the exact instrument release before starting a new session",
        ),
        AssessmentSessionStartError::LocaleMismatch => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:locale-mismatch",
            "Locale Mismatch",
            "start the session with the exact published release locale",
        ),
        AssessmentSessionStartError::InvalidStoredRelease
        | AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::InvalidStartRelease,
        ) => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:invalid-stored-release",
            "Invalid Stored Release",
            "repair the stored instrument release before starting a new session",
        ),
        AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay,
        ) => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different session create body",
        ),
        AssessmentSessionStartError::Persistence(_) => SessionHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:session-store-unavailable",
            "Session Store Unavailable",
            "retry the exact POST /v1/sessions after the session store is repaired",
        ),
    }
}

struct SessionCreateBody {
    participant_ref: String,
    instrument_release_ref: String,
    locale: String,
}

fn parse_create_body(body: &str) -> Option<SessionCreateBody> {
    let fields = parse_string_object(body)?;
    if fields.len() != 3 {
        return None;
    }
    Some(SessionCreateBody {
        participant_ref: fields.get("participant_ref")?.clone(),
        instrument_release_ref: fields.get("instrument_release_ref")?.clone(),
        locale: fields.get("locale")?.clone(),
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

fn session_body(session: &AssessmentSession) -> String {
    format!(
        "{{\"session_ref\":{},\"participant_ref\":{},\"instrument_release_ref\":{},\"instrument_version_ref\":{},\"instrument_release_content_digest\":{},\"locale\":{},\"state\":{},\"created_at_unix_ms\":{}}}",
        json_string(session.session_ref()),
        json_string(session.participant_ref()),
        json_string(session.instrument_release_ref()),
        json_string(session.instrument_version_ref()),
        json_string(session.instrument_release_content_digest()),
        json_string(session.locale()),
        json_string(session.state().persist_name()),
        session.created_at_unix_ms()
    )
}

fn valid_idempotency_key(value: &str) -> Option<&str> {
    normalized_reference(value.trim())
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
    if headers.is_empty() {
        None
    } else {
        Some(body)
    }
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || !version.starts_with("HTTP/") {
        None
    } else {
        Some((method, target))
    }
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
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

fn reject_full_request_buffer(filled: usize, capacity: usize) -> io::Result<()> {
    if filled == capacity {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session HTTP request exceeded the accepted size",
        ))
    } else {
        Ok(())
    }
}

fn reject_oversized_request(expected: usize, capacity: usize) -> io::Result<()> {
    if expected > capacity {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session HTTP request exceeded the accepted size",
        ))
    } else {
        Ok(())
    }
}

fn declared_request_end(body_start: usize, content_length: &str) -> io::Result<usize> {
    let content_length = content_length.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid session HTTP Content-Length",
        )
    })?;
    body_start.checked_add(content_length).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "session HTTP request exceeded the accepted size",
        )
    })
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<String> {
    let mut buffer = vec![0_u8; SESSION_HTTP_MAX_REQUEST_BYTES];
    let mut filled = 0;
    loop {
        reject_full_request_buffer(filled, buffer.len())?;
        let read = stream.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
        let Some(header_offset) = buffer[..filled]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        else {
            continue;
        };
        let body_start = header_offset + 4;
        let headers = std::str::from_utf8(&buffer[..body_start])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(value) = header_value(headers, "content-length") {
            let expected = declared_request_end(body_start, value)?;
            reject_oversized_request(expected, buffer.len())?;
            if filled < expected {
                continue;
            }
            filled = expected;
        }
        break;
    }
    decode_request_bytes(&buffer[..filled])
}

fn decode_request_bytes(bytes: &[u8]) -> io::Result<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_session_bytes(stream: &mut impl io::Write, bytes: &[u8]) -> io::Result<()> {
    stream.write_all(bytes)
}

fn write_http_response(
    stream: &mut impl io::Write,
    response: &SessionHttpResponse,
) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        status = response.status,
        reason = reason_phrase(response.status),
        content_type = response.content_type,
        len = response.body.len()
    );
    write_session_bytes(stream, header.as_bytes())?;
    write_session_bytes(stream, response.body.as_bytes())?;
    Ok(())
}

const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bind_session_http, declared_request_end, decode_request_bytes, handle_session_http_request,
        json_string, parse_create_body, parse_json_string, parse_request_line, parse_string_object,
        reason_phrase, reject_full_request_buffer, reject_oversized_request, request_body,
        split_target, valid_idempotency_key, write_http_response, write_session_bytes,
        MemorySessionHttpPort, PostgresSessionHttpPort, SessionHttpPort, SessionHttpResponse,
        DIGEST,
    };
    use crate::instrument::{
        InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
        PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    };
    use crate::postgres_assessment_session::{
        apply_assessment_session_migration, AssessmentSessionStartError,
    };
    use crate::postgres_instrument_release::{
        apply_instrument_release_migration, persist_instrument_release,
    };
    use postgres::{Client, NoTls};
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};

    struct FailWrite;

    struct FailAfterFirstWrite {
        writes: u8,
    }

    impl std::io::Write for FailWrite {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "flush fail",
            ))
        }
    }

    impl std::io::Write for FailAfterFirstWrite {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.writes == 0 {
                self.writes = 1;
                Ok(buf.len())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "second write fail",
                ))
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn helpers_cover_json_http_and_reason_edges() {
        assert_eq!(json_string("a\"b\\c\n\r\t"), "\"a\\\"b\\\\c\\n\\r\\t\"");
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(split_target("/v1/sessions?x=1"), ("/v1/sessions", "x=1"));
        assert!(parse_request_line("POST /v1/sessions HTTP/1.1 extra").is_none());
        assert!(parse_request_line("POST /v1/sessions SMTP/1.0").is_none());
        assert_eq!(
            parse_json_string(r#""a\"b\\c\n\r\td""#),
            Some((String::from("a\"b\\c\n\r\td"), ""))
        );
        assert!(parse_json_string(r#""\q""#).is_none());
        assert!(parse_json_string("\"\u{0001}\"").is_none());
        assert!(parse_json_string("\"unterminated").is_none());
        assert_eq!(parse_string_object("{}").unwrap().len(), 0);
        assert!(parse_string_object(r#"{"a":"1","a":"2"}"#).is_none());
        assert!(parse_string_object(r#"{"a":"1",}"#).is_none());
        assert!(parse_create_body("{}").is_none());
        assert!(valid_idempotency_key("12").is_none());
        assert_eq!(valid_idempotency_key(" ses_ok "), Some("ses_ok"));
        assert_eq!(request_body("\r\n\r\n{}"), None);
        assert_eq!(
            request_body("POST /v1/sessions HTTP/1.1\r\n\r\n{}"),
            Some("{}")
        );
        assert_eq!(declared_request_end(32, "8").unwrap(), 40);
        assert_eq!(
            declared_request_end(32, "no").unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(32, "18446744073709551615")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(reject_oversized_request(100, 8_192).is_ok());
        assert_eq!(
            reject_oversized_request(20_000, 8_192).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(reject_full_request_buffer(100, 8_192).is_ok());
        assert_eq!(
            reject_full_request_buffer(8_192, 8_192).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            decode_request_bytes(b"POST /v1/sessions").unwrap(),
            "POST /v1/sessions"
        );
        assert_eq!(
            decode_request_bytes(&[0xff, 0xfe]).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            write_session_bytes(&mut FailWrite, b"HTTP/1.1")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            std::io::Write::flush(&mut FailWrite).unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );
        let created = SessionHttpResponse {
            status: 201,
            content_type: "application/json",
            body: String::from("{\"ok\":true}"),
        };
        assert_eq!(
            write_http_response(&mut FailWrite, &created)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        let mut sink = Vec::new();
        write_http_response(&mut sink, &created).unwrap();
        assert!(String::from_utf8(sink).unwrap().starts_with("HTTP/1.1 201"));
        let mut second = FailAfterFirstWrite { writes: 0 };
        assert_eq!(
            write_http_response(&mut second, &created)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert!(std::io::Write::flush(&mut second).is_ok());
    }

    #[test]
    fn listener_waits_until_content_length_body_arrives() {
        use crate::session_http::{
            accept_one_session_http, handle_session_http_request, MemorySessionHttpPort,
        };
        use std::io::{Read, Write};
        use std::net::Shutdown;
        use std::time::Duration;

        let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let address = listener.local_addr().unwrap();
        let body = "{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        let headers = format!(
            "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_wait_body\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let server = std::thread::spawn(move || {
            let mut port = MemorySessionHttpPort::published();
            accept_one_session_http(&listener, &mut port, 20_000).unwrap();
            port
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(headers.as_bytes()).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        stream.write_all(body.as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let mut port = server.join().unwrap();
        assert!(
            response.starts_with("HTTP/1.1 201 Created\r\n"),
            "server must wait for the declared body before dispatch: {response}"
        );
        let replay = handle_session_http_request(
            &format!(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_wait_body\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
            &mut port,
            20_000,
        );
        assert_eq!(replay.status(), 200);
    }

    #[test]
    fn bind_accepts_one_loopback_create() {
        use crate::session_http::{
            accept_one_session_http, handle_session_http_request, MemorySessionHttpPort,
        };
        let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let addr = listener.local_addr().unwrap();
        let request = "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_loopback\r\n\r\n{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        let worker = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut body = String::new();
            std::io::Read::read_to_string(&mut stream, &mut body).unwrap();
            body
        });
        let mut port = MemorySessionHttpPort::published();
        accept_one_session_http(&listener, &mut port, 20_000).unwrap();
        let response = worker.join().unwrap();
        assert!(response.contains("201"));
        let replay = handle_session_http_request(request, &mut port, 20_000);
        assert_eq!(replay.status(), 200);
    }

    fn create_request(session_ref: &str, locale: &str) -> String {
        format!(
            "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: {session_ref}\r\n\r\n{{\
             \"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\
             \"instrument_release_ref\":\"release_big_five_ko_v1\",\
             \"locale\":\"{locale}\"}}"
        )
    }

    #[test]
    fn session_http_routing_problems_instantiate_in_the_library() {
        use crate::session_http::{handle_session_http_request, MemorySessionHttpPort};

        let mut port = MemorySessionHttpPort::published();
        assert_eq!(
            handle_session_http_request("NOTHTTP", &mut port, 20_000).status(),
            400
        );
        assert_eq!(
            handle_session_http_request("DELETE /v1/sessions HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            405
        );
        assert_eq!(
            handle_session_http_request(
                "DELETE /v1/sessions/ses_x HTTP/1.1\r\n\r\n",
                &mut port,
                20_000
            )
            .status(),
            405
        );
        assert_eq!(
            handle_session_http_request("GET /v1/other HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            404
        );
        assert_eq!(
            handle_session_http_request("PUT /v1/instruments HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            404
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_nobody",
                &mut port,
                20_000
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request("POST /v1/sessions HTTP/1.1\r\n\r\n{}", &mut port, 20_000)
                .status(),
            400
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_ok\r\n\r\n",
                &mut port,
                20_000
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_ok\r\n\r\n{}",
                &mut port,
                20_000
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request("GET /v1/sessions HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            405
        );
        assert_eq!(
            handle_session_http_request("GET /v1/sessions/ HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            404
        );
        assert_eq!(
            handle_session_http_request(
                "GET /v1/sessions/ses_x/commands HTTP/1.1\r\n\r\n",
                &mut port,
                20_000
            )
            .status(),
            404
        );
        assert_eq!(
            handle_session_http_request(
                "GET /v1/sessions/ses_missing HTTP/1.1\r\n\r\n",
                &mut port,
                20_000
            )
            .status(),
            404
        );
    }

    #[test]
    fn session_http_start_and_load_problems_instantiate_in_the_library() {
        use crate::postgres_assessment_session::{
            AssessmentSessionPersistenceError, AssessmentSessionStartError,
        };
        use crate::session_http::{handle_session_http_request, MemorySessionHttpPort};

        let mut port = MemorySessionHttpPort::published();
        assert_eq!(
            handle_session_http_request(&create_request("ses_locale", "en-US"), &mut port, 20_000)
                .status(),
            409
        );
        port.published = false;
        assert_eq!(
            handle_session_http_request(&create_request("ses_unpub", "ko-KR"), &mut port, 20_000)
                .status(),
            409
        );
        port.published = true;
        let created = handle_session_http_request(
            &create_request("ses_conflict", "ko-KR"),
            &mut port,
            20_000,
        );
        assert_eq!(created.status(), 201);
        let conflict = handle_session_http_request(
            "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_conflict\r\n\r\n{\"participant_ref\":\"ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}",
            &mut port,
            20_000,
        );
        assert_eq!(conflict.status(), 409);
        assert!(conflict.body().contains("idempotency-conflict"));

        for (error, expected) in [
            (AssessmentSessionStartError::InvalidReference, 400),
            (AssessmentSessionStartError::InvalidTimestamp, 500),
            (
                AssessmentSessionStartError::InstrumentReleaseUnavailable,
                409,
            ),
            (AssessmentSessionStartError::LocaleMismatch, 409),
            (AssessmentSessionStartError::InvalidStoredRelease, 409),
            (
                AssessmentSessionStartError::Persistence(
                    AssessmentSessionPersistenceError::UnpublishedStart,
                ),
                409,
            ),
            (
                AssessmentSessionStartError::Persistence(
                    AssessmentSessionPersistenceError::InvalidStartRelease,
                ),
                409,
            ),
            (
                AssessmentSessionStartError::Persistence(
                    AssessmentSessionPersistenceError::ConflictingReplay,
                ),
                409,
            ),
            (
                AssessmentSessionStartError::Persistence(
                    AssessmentSessionPersistenceError::UnsupportedIsolationLevel,
                ),
                500,
            ),
        ] {
            port.next_start_error = Some(error);
            assert_eq!(
                handle_session_http_request(&create_request("ses_map", "ko-KR"), &mut port, 20_000)
                    .status(),
                expected
            );
        }
        assert_eq!(
            handle_session_http_request("GET /v1/sessions/12 HTTP/1.1\r\n\r\n", &mut port, 20_000)
                .status(),
            400
        );
        port.next_load_error = Some(AssessmentSessionPersistenceError::InvalidStoredIdentity);
        assert_eq!(
            handle_session_http_request(
                "GET /v1/sessions/ses_broken HTTP/1.1\r\n\r\n",
                &mut port,
                20_000
            )
            .status(),
            500
        );
    }

    #[test]
    fn session_http_reader_rejects_oversized_and_overflow_lengths() {
        use crate::session_http::{
            accept_one_session_http, bind_session_http, MemorySessionHttpPort,
        };
        use std::io::{Read, Write};
        use std::net::{Shutdown, SocketAddr, TcpStream};

        fn framing_kind(payload: &[u8]) -> std::io::ErrorKind {
            let listener = bind_session_http(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
            let address = listener.local_addr().unwrap();
            let body = payload.to_vec();
            let client = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                let _ = stream.write_all(&body);
                let _ = stream.shutdown(Shutdown::Write);
                let mut response = String::new();
                let _ = stream.read_to_string(&mut response);
            });
            let mut port = MemorySessionHttpPort::published();
            let error = accept_one_session_http(&listener, &mut port, 20_000).unwrap_err();
            client.join().unwrap();
            error.kind()
        }

        let huge_header = format!(
            "POST /v1/sessions HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(9000)
        );
        assert_eq!(
            framing_kind(huge_header.as_bytes()),
            std::io::ErrorKind::InvalidData
        );
        let oversized = format!(
            "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_huge\r\nContent-Length: 20000\r\n\r\n{}",
            "x".repeat(100)
        );
        assert_eq!(
            framing_kind(oversized.as_bytes()),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            framing_kind(
                b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_overflow\r\nContent-Length: 18446744073709551615\r\n\r\n"
            ),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            framing_kind(
                b"POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_bad_utf8\r\nX-Bad: \xff\r\n\r\n"
            ),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn library_create_reloads_over_get_and_rejects_numeric_start_identity() {
        let mut port = MemorySessionHttpPort::published();
        let created = handle_session_http_request(
            &create_request("ses_lib_reload", "ko-KR"),
            &mut port,
            20_000,
        );
        assert_eq!(created.status(), 201);
        assert_eq!(created.content_type(), "application/json");
        let loaded = handle_session_http_request(
            "GET /v1/sessions/ses_lib_reload HTTP/1.1\r\n\r\n",
            &mut port,
            20_000,
        );
        assert_eq!(loaded.status(), 200);
        assert_eq!(loaded.content_type(), "application/json");
        assert_eq!(loaded.body(), created.body());

        let colonless = handle_session_http_request(
            "POST /v1/sessions HTTP/1.1\r\nX-Trace-Id\r\nIdempotency-Key: ses_nocolon\r\n\r\n{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}",
            &mut port,
            20_000,
        );
        assert_eq!(colonless.status(), 201);
        assert_eq!(colonless.content_type(), "application/json");

        assert!(matches!(
            port.start_from_stored_release(
                "12",
                "ptc_eb1b318917d24ca0ac5153c37ff696c7",
                "release_big_five_ko_v1",
                "ko-KR",
                21_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
    }

    fn published_release_for_http_port() -> InstrumentRelease {
        let mut published = InstrumentRelease::new(
            InstrumentReleaseManifest::new(
                "release_big_five_ko_v1",
                "instrument_big_five",
                "instrument_version_big_five_ko_v1",
                "construct_big_five",
                &["item_version_001", "item_version_002"],
                "ko-KR",
                "assessment_spec_big_five_v1",
                "scoring_version_big_five_v1",
                "calibration_big_five_ko_v1",
                Some("norm_version_big_five_ko_v1"),
                "narrative_version_big_five_v1",
                &["consent_service_v1"],
                "intended_use_self_reflection_v1",
                "limitations_nonclinical_v1",
                DIGEST,
            )
            .unwrap(),
            10_000,
        )
        .unwrap();
        published
            .apply_command(
                "publication_review_http_port",
                PublicationCommand::SubmitReview,
                10_100,
            )
            .unwrap();
        published
            .bind_publication_evidence(
                PublicationEvidenceRecord::new(
                    "publication_evidence_big_five_ko_v1",
                    "evidence_policy_self_reflection_v1",
                    "release_big_five_ko_v1",
                    "instrument_version_big_five_ko_v1",
                    &["item_version_001", "item_version_002"],
                    DIGEST,
                    "ko-KR",
                    "intended_use_self_reflection_v1",
                    "assessment_spec_big_five_v1",
                    "scoring_version_big_five_v1",
                    "calibration_big_five_ko_v1",
                    Some("norm_version_big_five_ko_v1"),
                    "limitations_nonclinical_v1",
                    PublicationEvidenceProvenance::new(
                        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                        "population_general_adult_v1",
                        "administration_web_self_report_v1",
                        "measurement_model_big_five_v1",
                        10_050,
                        None,
                    )
                    .unwrap(),
                    &["rights_ipip_big_five_v1"],
                    &["recovery_big_five_ko_v1"],
                    &["approval_psychometrics_big_five_ko_v1"],
                    PublicationEvidenceStatus::Approved,
                )
                .unwrap(),
            )
            .unwrap();
        published
            .apply_command(
                "publication_publish_http_port",
                PublicationCommand::Publish,
                10_200,
            )
            .unwrap();
        published
    }

    #[test]
    fn persist_backed_http_port_starts_and_reloads_from_the_library() {
        let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
        let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
        client
            .batch_execute(
                "DROP SCHEMA IF EXISTS session_http_port_library CASCADE; \
                 CREATE SCHEMA session_http_port_library; \
                 SET search_path TO session_http_port_library;",
            )
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        apply_assessment_session_migration(&mut client).unwrap();
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &published_release_for_http_port()).unwrap();
        transaction.commit().unwrap();

        let mut transaction = client.transaction().unwrap();
        let created = {
            let mut port = PostgresSessionHttpPort::new(&mut transaction);
            handle_session_http_request(
                &create_request("ses_port_library", "ko-KR"),
                &mut port,
                20_000,
            )
        };
        assert_eq!(created.status(), 201);
        assert_eq!(created.content_type(), "application/json");
        transaction.commit().unwrap();

        let mut transaction = client.transaction().unwrap();
        let loaded = {
            let mut port = PostgresSessionHttpPort::new(&mut transaction);
            handle_session_http_request(
                "GET /v1/sessions/ses_port_library HTTP/1.1\r\n\r\n",
                &mut port,
                20_000,
            )
        };
        assert_eq!(loaded.status(), 200);
        assert_eq!(loaded.content_type(), "application/json");
        assert_eq!(loaded.body(), created.body());
        transaction.commit().unwrap();
    }
}
