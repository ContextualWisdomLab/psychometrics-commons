//! Public HTTP transport for creating and reloading assessment sessions.
//!
//! This slice exposes `POST /v1/sessions` and `GET /v1/sessions/{session_ref}`
//! over HTTP/1.1. The server mints the session reference, pins one published
//! locale-specific instrument release, and treats `Idempotency-Key` as the
//! replay contract. Sessions live in process memory; `PostgreSQL` session
//! durability remains a later persist slice. Errors use RFC 9457 problem
//! details and never echo raw request bodies, SQL, or provider text.

use crate::instrument::InstrumentRelease;
use crate::session::{AssessmentSession, SessionCreationError, SessionState};
use std::collections::HashMap;
use std::fmt::Write;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Public collection path for assessment sessions.
pub const SESSION_COLLECTION_PATH: &str = "/v1/sessions";
/// Bounded read/write timeout for one accepted session HTTP connection.
pub const SESSION_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted session HTTP request size, including headers and body.
pub const SESSION_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

/// In-process catalog, minted identity, and idempotent create store.
pub struct SessionHttpRuntime {
    releases: HashMap<String, InstrumentRelease>,
    sessions: HashMap<String, AssessmentSession>,
    idempotency: HashMap<String, IdempotentCreate>,
    next_session_ref: String,
    created_at_unix_ms: u64,
}

struct IdempotentCreate {
    fingerprint: String,
    session_ref: String,
}

impl SessionHttpRuntime {
    /// Create a runtime that can mint one known next session reference.
    ///
    /// `releases` is the exact published-or-unpublished catalog the handler may
    /// bind. `next_session_ref` is the opaque identity the next successful
    /// create will mint. `created_at_unix_ms` is the server clock used for
    /// [`AssessmentSession::new`].
    #[must_use]
    pub fn new(
        releases: Vec<InstrumentRelease>,
        next_session_ref: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Self {
        let releases = releases
            .into_iter()
            .map(|release| (release.manifest().release_ref().to_owned(), release))
            .collect();
        Self {
            releases,
            sessions: HashMap::new(),
            idempotency: HashMap::new(),
            next_session_ref: next_session_ref.into(),
            created_at_unix_ms,
        }
    }

    /// Replace the next minted session reference after a successful create.
    pub fn replace_next_session_ref(&mut self, next_session_ref: impl Into<String>) {
        self.next_session_ref = next_session_ref.into();
    }

    /// Return one stored session by opaque reference.
    #[must_use]
    pub fn session(&self, session_ref: &str) -> Option<&AssessmentSession> {
        self.sessions.get(session_ref)
    }

    /// Return how many sessions this process currently holds.
    #[must_use]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
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

/// Translate one raw HTTP/1.1 request into a session create or reload response.
///
/// Unknown methods, paths, JSON shapes, missing idempotency keys, unpublished
/// releases, and locale mismatches fail closed with RFC 9457 problem details.
/// Exact idempotent create replay returns the original session without minting
/// a second identity.
#[must_use]
pub fn handle_session_http_request(
    request: &str,
    runtime: &mut SessionHttpRuntime,
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
        ("POST", SESSION_COLLECTION_PATH) => handle_create(request, runtime),
        ("GET", path) => handle_get(path, runtime),
        (_, SESSION_COLLECTION_PATH) => SessionHttpResponse::problem(
            405,
            "urn:psychometrics-commons:problem:method-not-allowed",
            "Method Not Allowed",
            "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
        ),
        (_, path) if path.starts_with("/v1/sessions/") => SessionHttpResponse::problem(
            405,
            "urn:psychometrics-commons:problem:method-not-allowed",
            "Method Not Allowed",
            "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
        ),
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
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_session_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single session HTTP request.
///
/// The connection is closed after the response. Keep-alive, TLS, and other
/// public families are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_session_http(
    listener: &TcpListener,
    runtime: &mut SessionHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(SESSION_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(SESSION_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_session_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn handle_create(request: &str, runtime: &mut SessionHttpRuntime) -> SessionHttpResponse {
    let Some(idempotency_key) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/sessions requires an opaque Idempotency-Key header",
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
    let fingerprint = format!(
        "{}\n{}\n{}",
        create.participant_ref, create.instrument_release_ref, create.locale
    );
    if let Some(existing) = runtime.idempotency.get(idempotency_key) {
        if existing.fingerprint == fingerprint {
            let session = runtime
                .sessions
                .get(&existing.session_ref)
                .expect("idempotent create stores the minted session");
            return SessionHttpResponse::json(200, session_body(session));
        }
        return SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different session create body",
        );
    }
    mint_created_session(runtime, idempotency_key, fingerprint, &create)
}

fn mint_created_session(
    runtime: &mut SessionHttpRuntime,
    idempotency_key: &str,
    fingerprint: String,
    create: &SessionCreateBody,
) -> SessionHttpResponse {
    let Some(release) = runtime.releases.get(&create.instrument_release_ref) else {
        return SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:instrument-release-not-found",
            "Instrument Release Not Found",
            "session create requires a cataloged instrument release",
        );
    };
    match AssessmentSession::new(
        &runtime.next_session_ref,
        &create.participant_ref,
        release,
        &create.locale,
        runtime.created_at_unix_ms,
    ) {
        Ok(session) => store_created_session(runtime, idempotency_key, fingerprint, session),
        Err(error) => creation_problem(error),
    }
}

fn store_created_session(
    runtime: &mut SessionHttpRuntime,
    idempotency_key: &str,
    fingerprint: String,
    session: AssessmentSession,
) -> SessionHttpResponse {
    let session_ref = session.session_ref().to_owned();
    runtime.sessions.insert(session_ref.clone(), session);
    runtime.idempotency.insert(
        idempotency_key.to_owned(),
        IdempotentCreate {
            fingerprint,
            session_ref: session_ref.clone(),
        },
    );
    let stored = runtime
        .sessions
        .get(&session_ref)
        .expect("create inserts the minted session");
    SessionHttpResponse::json(201, session_body(stored))
}

fn creation_problem(error: SessionCreationError) -> SessionHttpResponse {
    match error {
        SessionCreationError::InvalidReference => SessionHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "session and participant references must be opaque non-numeric values",
        ),
        SessionCreationError::InvalidTimestamp => SessionHttpResponse::problem(
            500,
            "urn:psychometrics-commons:problem:invalid-server-timestamp",
            "Invalid Server Timestamp",
            "session create requires a positive server clock",
        ),
        SessionCreationError::InstrumentReleaseUnavailable => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:instrument-release-unavailable",
            "Instrument Release Unavailable",
            "session create requires an instrument release currently published for new sessions",
        ),
        SessionCreationError::LocaleMismatch => SessionHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:locale-mismatch",
            "Locale Mismatch",
            "requested locale must exactly match the published instrument release locale",
        ),
    }
}

fn handle_get(path: &str, runtime: &SessionHttpRuntime) -> SessionHttpResponse {
    let Some(session_ref) = path
        .strip_prefix("/v1/sessions/")
        .filter(|value| !value.is_empty() && !value.contains('/'))
    else {
        if path == SESSION_COLLECTION_PATH {
            return SessionHttpResponse::problem(
                405,
                "urn:psychometrics-commons:problem:method-not-allowed",
                "Method Not Allowed",
                "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
            );
        }
        return SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "session routes accept POST /v1/sessions and GET /v1/sessions/{session_ref} only",
        );
    };
    match runtime.sessions.get(session_ref) {
        Some(session) => SessionHttpResponse::json(200, session_body(session)),
        None => SessionHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:session-not-found",
            "Session Not Found",
            "GET /v1/sessions/{session_ref} requires a session created in this process",
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
        json_string(session_state_label(session.state())),
        session.created_at_unix_ms()
    )
}

const fn session_state_label(state: SessionState) -> &'static str {
    match state {
        SessionState::Created => "created",
        SessionState::Active => "active",
        SessionState::Paused => "paused",
        SessionState::Completed => "completed",
        SessionState::Scoring => "scoring",
        SessionState::Scored => "scored",
        SessionState::Released => "released",
        SessionState::Expired => "expired",
        SessionState::Cancelled => "cancelled",
        SessionState::Invalidated => "invalidated",
    }
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
        (header_name.eq_ignore_ascii_case(name)).then(|| value.trim())
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
    if buffer.len() > SESSION_HTTP_MAX_REQUEST_BYTES
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
                || buffer.len() > SESSION_HTTP_MAX_REQUEST_BYTES
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

fn write_http_response(stream: &mut TcpStream, response: &SessionHttpResponse) -> io::Result<()> {
    let body = response.body().as_bytes();
    let allow = if response.status() == 405 {
        "Allow: GET, POST\r\n"
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
        500 => "Internal Server Error",
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
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::{
        apply_request_read, handle_session_http_request, json_string, parse_create_body,
        parse_json_string, parse_string_object, reason_phrase, session_state_label,
        valid_idempotency_key, RequestReadProgress, SessionHttpRuntime, SESSION_COLLECTION_PATH,
        SESSION_HTTP_MAX_REQUEST_BYTES,
    };
    use crate::instrument::{
        InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
        PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    };
    use crate::session::SessionState;
    use std::io::{self, ErrorKind};

    const VALID_DIGEST: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const EVIDENCE_DIGEST: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn manifest() -> InstrumentReleaseManifest {
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
            VALID_DIGEST,
        )
        .unwrap()
    }

    fn approved_evidence() -> PublicationEvidenceRecord {
        PublicationEvidenceRecord::new(
            "publication_evidence_big_five_ko_v1",
            "evidence_policy_self_reflection_v1",
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            &["item_version_001", "item_version_002"],
            VALID_DIGEST,
            "ko-KR",
            "intended_use_self_reflection_v1",
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_ko_v1",
            Some("norm_version_big_five_ko_v1"),
            "limitations_nonclinical_v1",
            PublicationEvidenceProvenance::new(
                EVIDENCE_DIGEST,
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
        .unwrap()
    }

    fn published_release() -> InstrumentRelease {
        let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
        release
            .apply_command(
                "publication_review_f9f86084",
                PublicationCommand::SubmitReview,
                10_100,
            )
            .unwrap();
        release
            .bind_publication_evidence(approved_evidence())
            .unwrap();
        release
            .apply_command(
                "publication_publish_635a7491",
                PublicationCommand::Publish,
                10_200,
            )
            .unwrap();
        release
    }

    fn runtime() -> SessionHttpRuntime {
        SessionHttpRuntime::new(
            vec![published_release()],
            "ses_unit_next",
            1_725_000_000_000,
        )
    }

    #[test]
    fn remaining_labels_escapes_and_parse_edges_are_stable() {
        assert_eq!(session_state_label(SessionState::Active), "active");
        assert_eq!(session_state_label(SessionState::Paused), "paused");
        assert_eq!(session_state_label(SessionState::Completed), "completed");
        assert_eq!(session_state_label(SessionState::Scoring), "scoring");
        assert_eq!(session_state_label(SessionState::Scored), "scored");
        assert_eq!(session_state_label(SessionState::Released), "released");
        assert_eq!(session_state_label(SessionState::Expired), "expired");
        assert_eq!(session_state_label(SessionState::Cancelled), "cancelled");
        assert_eq!(
            session_state_label(SessionState::Invalidated),
            "invalidated"
        );
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(valid_idempotency_key("  "), None);
        assert_eq!(valid_idempotency_key("has space"), None);
        assert_eq!(valid_idempotency_key("12345"), None);
        assert_eq!(valid_idempotency_key("idem_ok"), Some("idem_ok"));
        assert!(parse_string_object("{").is_none());
        assert!(parse_string_object("{\"a\":\"1\",\"a\":\"2\"}").is_none());
        assert!(parse_string_object("{\"a\":\"1\",}").is_none());
        assert!(parse_create_body("{\"participant_ref\":\"p\"}").is_none());
        assert!(parse_json_string("\"unterminated").is_none());
        assert!(parse_json_string("\"bad\\x\"").is_none());
        assert!(parse_json_string("\"\u{0001}\"").is_none());
        let (decoded, rest) = parse_json_string("\"a\\\"b\\\\c\\n\\r\\t\"tail").unwrap();
        assert_eq!(decoded, "a\"b\\c\n\r\t");
        assert_eq!(rest, "tail");
    }

    #[test]
    fn request_read_progress_and_transport_failures_are_classified() {
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(&mut Vec::new(), b"GET", Ok(3)).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; SESSION_HTTP_MAX_REQUEST_BYTES];
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
    fn handler_covers_method_path_body_and_identity_failures() {
        let mut runtime = runtime();
        assert_eq!(
            handle_session_http_request("NOT-A-REQUEST", &mut runtime).status(),
            400
        );
        assert_eq!(
            handle_session_http_request("GET /v1/sessions HTTP/1.1\r\n\r\n", &mut runtime).status(),
            405
        );
        assert_eq!(
            handle_session_http_request("PUT /v1/sessions HTTP/1.1\r\n\r\n", &mut runtime).status(),
            405
        );
        assert_eq!(
            handle_session_http_request("POST /v1/sessions/ses_x HTTP/1.1\r\n\r\n", &mut runtime)
                .status(),
            405
        );
        assert_eq!(
            handle_session_http_request("DELETE /v1/sessions/ses_x HTTP/1.1\r\n\r\n", &mut runtime)
                .status(),
            405
        );
        assert_eq!(
            handle_session_http_request("GET /v1/results/r1 HTTP/1.1\r\n\r\n", &mut runtime)
                .status(),
            404
        );
        assert_eq!(
            handle_session_http_request(
                &format!("POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\r\n{{}}"),
                &mut runtime
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: 99\r\nContent-Length: 2\r\n\r\n{}",
                &mut runtime
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_ok\r\nContent-Length: 2\r\n\r\n{}",
                &mut runtime
            )
            .status(),
            400
        );
        assert_eq!(
            handle_session_http_request(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_ok\r\nContent-Length: 8\r\n\r\nshort",
                &mut runtime
            )
            .status(),
            400
        );
    }

    #[test]
    fn handler_covers_catalog_identity_and_get_failures() {
        let mut runtime = runtime();
        let unknown_release = "{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_missing\",\"locale\":\"ko-KR\"}";
        assert_eq!(
            handle_session_http_request(
                &format!(
                    "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_missing\r\nContent-Length: {}\r\n\r\n{unknown_release}",
                    unknown_release.len()
                ),
                &mut runtime
            )
            .status(),
            404
        );
        let invalid_participant = "{\"participant_ref\":\"12345\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        assert_eq!(
            handle_session_http_request(
                &format!(
                    "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_bad_ref\r\nContent-Length: {}\r\n\r\n{invalid_participant}",
                    invalid_participant.len()
                ),
                &mut runtime
            )
            .status(),
            400
        );
        let mut zero_clock = SessionHttpRuntime::new(vec![published_release()], "ses_zero", 0);
        let valid = "{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        assert_eq!(
            handle_session_http_request(
                &format!(
                    "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_zero\r\nContent-Length: {}\r\n\r\n{valid}",
                    valid.len()
                ),
                &mut zero_clock
            )
            .status(),
            500
        );
        assert_eq!(
            handle_session_http_request(
                "GET /v1/sessions/ses_missing HTTP/1.1\r\n\r\n",
                &mut runtime
            )
            .status(),
            404
        );
        assert_eq!(
            handle_session_http_request(
                "GET /v1/sessions/ses_x/extra HTTP/1.1\r\n\r\n",
                &mut runtime
            )
            .status(),
            404
        );
    }

    #[test]
    fn conflicting_idempotency_and_get_after_create_are_stable() {
        let mut runtime = runtime();
        let valid = "{\"participant_ref\":\"ptc_eb1b318917d24ca0ac5153c37ff696c7\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        let created = handle_session_http_request(
            &format!(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_shared\r\nContent-Length: {}\r\n\r\n{valid}",
                valid.len()
            ),
            &mut runtime,
        );
        assert_eq!(created.status(), 201);
        let other = "{\"participant_ref\":\"ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}";
        let conflict = handle_session_http_request(
            &format!(
                "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: idem_shared\r\nContent-Length: {}\r\n\r\n{other}",
                other.len()
            ),
            &mut runtime,
        );
        assert_eq!(conflict.status(), 409);
        assert!(conflict
            .body()
            .contains("urn:psychometrics-commons:problem:idempotency-conflict"));
        let loaded = handle_session_http_request(
            "GET /v1/sessions/ses_unit_next HTTP/1.1\r\n\r\n",
            &mut runtime,
        );
        assert_eq!(loaded.status(), 200);
        assert_eq!(loaded.body(), created.body());
    }
}
