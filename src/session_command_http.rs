//! Public HTTP transport for participant session lifecycle commands.
//!
//! This slice exposes `POST /v1/sessions/{session_ref}/commands` over HTTP/1.1.
//! A purchaser may Activate, Pause, Resume, Complete, or Cancel an injected
//! session. The `Idempotency-Key` header is the opaque command reference.
//! Sessions are not created here; catalog list, session create, item delivery,
//! response write, scoring, and persistence remain other families. Errors use
//! RFC 9457 problem details and never echo raw request bodies or store text
//! (Nottingham et al., 2023).

use crate::reference::normalized_reference;
use crate::session::{
    AssessmentSession, SessionCommand, SessionState, TransitionError, TransitionErrorKind,
};
use std::collections::HashMap;
use std::fmt::Write;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Bounded read/write timeout for one accepted session-command HTTP connection.
pub const SESSION_COMMAND_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted session-command HTTP request size, including headers and body.
pub const SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

/// In-process sessions that can receive participant lifecycle commands.
pub struct SessionCommandHttpRuntime {
    sessions: HashMap<String, AssessmentSession>,
}

impl SessionCommandHttpRuntime {
    /// Create a runtime from injected sessions.
    ///
    /// Created, paused, completed, and other states may be supplied for operator
    /// tests; the handler still rejects commands that are illegal from that state.
    #[must_use]
    pub fn new(sessions: Vec<AssessmentSession>) -> Self {
        let sessions = sessions
            .into_iter()
            .map(|session| (session.session_ref().to_owned(), session))
            .collect();
        Self { sessions }
    }

    /// Return one stored session by opaque reference.
    #[must_use]
    pub fn session(&self, session_ref: &str) -> Option<&AssessmentSession> {
        self.sessions.get(session_ref)
    }
}

/// HTTP response produced by a public session-command request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCommandHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl SessionCommandHttpResponse {
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

/// Translate one raw HTTP/1.1 request into a session lifecycle command.
///
/// Unknown methods, encoded or numeric session references, missing idempotency
/// keys, scoring or operator commands, and illegal transitions fail closed with
/// RFC 9457 problem details that tell the purchaser the next legal action.
#[must_use]
pub fn handle_session_command_http_request(
    request: &str,
    runtime: &mut SessionCommandHttpRuntime,
) -> SessionCommandHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session command request must include an HTTP method and target",
        );
    };
    let path = split_target(target).0;
    match (method, command_collection_session_ref(path)) {
        ("POST", Some(session_ref)) => handle_post(request, session_ref, runtime),
        (_, Some(_)) => method_not_allowed(),
        _ => SessionCommandHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "session command routes accept POST /v1/sessions/{session_ref}/commands only",
        ),
    }
}

/// Bind a blocking TCP listener for public session-command HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_session_command_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single session-command HTTP request.
///
/// The connection is closed after the response. Keep-alive, TLS, and other
/// public families are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_session_command_http(
    listener: &TcpListener,
    runtime: &mut SessionCommandHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(SESSION_COMMAND_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(SESSION_COMMAND_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_session_command_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn method_not_allowed() -> SessionCommandHttpResponse {
    SessionCommandHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "session command routes accept POST /v1/sessions/{session_ref}/commands only",
    )
}

fn handle_post(
    request: &str,
    session_ref: &str,
    runtime: &mut SessionCommandHttpRuntime,
) -> SessionCommandHttpResponse {
    if session_ref.contains('%')
        || session_ref.chars().any(char::is_whitespace)
        || normalized_reference(session_ref).is_none()
    {
        return SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session_ref must be an opaque non-numeric session identity",
        );
    }
    let Some(command_ref) =
        header_value(request, "idempotency-key").and_then(valid_idempotency_key)
    else {
        return SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/sessions/{session_ref}/commands requires an opaque Idempotency-Key header",
        );
    };
    let Some(body) = request_body(request) else {
        return SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session command requires a JSON object body",
        );
    };
    match parse_public_command(body) {
        CommandParse::Public(command) => apply_public_command(runtime, session_ref, command_ref, command),
        CommandParse::ScoringOrOperator => SessionCommandHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:command-not-public",
            "Command Not Public",
            "Use activate, pause, resume, complete, or cancel. Scoring and operator commands stay on their own families after Complete",
        ),
        CommandParse::Invalid => SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "session command requires a command string of activate, pause, resume, complete, or cancel",
        ),
    }
}

fn apply_public_command(
    runtime: &mut SessionCommandHttpRuntime,
    session_ref: &str,
    command_ref: &str,
    command: SessionCommand,
) -> SessionCommandHttpResponse {
    let Some(session) = runtime.sessions.get_mut(session_ref) else {
        return SessionCommandHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:session-not-found",
            "Session Not Found",
            "Use GET /v1/sessions/{session_ref} to confirm the session exists, then POST /v1/sessions/{session_ref}/commands",
        );
    };
    match session.apply_client_command(command_ref, command) {
        Ok((state, sequence)) => SessionCommandHttpResponse::json(
            200,
            command_body(session_ref, command_ref, command, sequence, state),
        ),
        Err(error) => transition_problem(error),
    }
}

fn transition_problem(error: TransitionError) -> SessionCommandHttpResponse {
    match error.kind() {
        TransitionErrorKind::InvalidReference => SessionCommandHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "session command reference must be an opaque non-numeric identity",
        ),
        TransitionErrorKind::InvalidSequence => SessionCommandHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:invalid-sequence",
            "Invalid Sequence",
            "Retry the same Idempotency-Key; the server assigns the next command sequence",
        ),
        TransitionErrorKind::ConflictingReplay => SessionCommandHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Idempotency-Key was reused with a different command",
        ),
        TransitionErrorKind::InvalidTransition => SessionCommandHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:illegal-session-command",
            "Illegal Session Command",
            illegal_transition_next_action(error.state(), error.command()),
        ),
    }
}

const fn illegal_transition_next_action(
    state: SessionState,
    command: SessionCommand,
) -> &'static str {
    match (state, command) {
        (SessionState::Created, SessionCommand::Pause | SessionCommand::Resume) => {
            "POST activate before pause or resume"
        }
        (SessionState::Created, SessionCommand::Complete) => {
            "POST activate, record every published item, then Complete"
        }
        (SessionState::Paused, SessionCommand::Complete) => {
            "POST resume, finish remaining items, then Complete"
        }
        (SessionState::Completed | SessionState::Scoring | SessionState::Scored, _) => {
            "This session is already past response collection. Use GET /v1/results/{result_ref} after scoring releases a snapshot"
        }
        (SessionState::Cancelled | SessionState::Expired | SessionState::Invalidated, _) => {
            "This session cannot accept more commands. Start a new session from a published release"
        }
        _ => "Use a command that is legal from the current session state",
    }
}

const fn next_action(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => {
            "POST /v1/sessions/{session_ref}/responses for each published item, or POST pause to take a break"
        }
        SessionState::Paused => {
            "POST resume when the participant is ready, then continue responses"
        }
        SessionState::Completed => {
            "Do not post more responses. Wait for scoring, then GET /v1/results/{result_ref}"
        }
        SessionState::Cancelled => "Do not post responses. Start a new session if the participant should try again",
        SessionState::Created => "POST activate to begin the assessment",
        SessionState::Scoring | SessionState::Scored | SessionState::Released => {
            "Use GET /v1/results/{result_ref} after the scored snapshot is released"
        }
        SessionState::Expired | SessionState::Invalidated => {
            "Start a new session from a published release"
        }
    }
}

enum CommandParse {
    Public(SessionCommand),
    ScoringOrOperator,
    Invalid,
}

fn parse_public_command(body: &str) -> CommandParse {
    let Some(fields) = parse_string_object(body) else {
        return CommandParse::Invalid;
    };
    if fields.len() != 1 {
        return CommandParse::Invalid;
    }
    match fields.get("command").map(String::as_str) {
        Some("activate") => CommandParse::Public(SessionCommand::Activate),
        Some("pause") => CommandParse::Public(SessionCommand::Pause),
        Some("resume") => CommandParse::Public(SessionCommand::Resume),
        Some("complete") => CommandParse::Public(SessionCommand::Complete),
        Some("cancel") => CommandParse::Public(SessionCommand::Cancel),
        Some("begin_scoring" | "record_score" | "release" | "expire" | "invalidate") => {
            CommandParse::ScoringOrOperator
        }
        _ => CommandParse::Invalid,
    }
}

fn command_body(
    session_ref: &str,
    command_ref: &str,
    command: SessionCommand,
    sequence: u64,
    state: SessionState,
) -> String {
    format!(
        "{{\"session_ref\":{},\"command_ref\":{},\"command\":{},\"sequence\":{sequence},\"state\":{},\"next_action\":{}}}",
        json_string(session_ref),
        json_string(command_ref),
        json_string(command.as_str()),
        json_string(state.as_str()),
        json_string(next_action(state))
    )
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

fn command_collection_session_ref(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v1/sessions/")?;
    let (session_ref, tail) = rest.split_once('/')?;
    (tail == "commands" && !session_ref.is_empty()).then_some(session_ref)
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
    if buffer.len() > SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES
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
                || buffer.len() > SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES
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

fn write_http_response(
    stream: &mut TcpStream,
    response: &SessionCommandHttpResponse,
) -> io::Result<()> {
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
        apply_request_read, command_collection_session_ref, illegal_transition_next_action,
        json_string, next_action, parse_public_command, reason_phrase, split_target,
        transition_problem, valid_idempotency_key, CommandParse, RequestReadProgress,
        SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES,
    };
    use crate::session::{SessionCommand, SessionState, TransitionError, TransitionErrorKind};
    use std::io::{self, ErrorKind};

    #[test]
    fn helpers_cover_paths_status_and_escapes() {
        assert_eq!(
            command_collection_session_ref("/v1/sessions/ses_one/commands"),
            Some("ses_one")
        );
        assert_eq!(
            command_collection_session_ref("/v1/sessions/ses_one/responses"),
            None
        );
        assert_eq!(command_collection_session_ref("/v1/sessions"), None);
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(
            split_target("/v1/sessions/ses_one/commands?x=1"),
            ("/v1/sessions/ses_one/commands", "x=1")
        );
        assert_eq!(valid_idempotency_key("idem_ok"), Some("idem_ok"));
        assert_eq!(valid_idempotency_key("42"), None);
        assert_eq!(valid_idempotency_key("   "), None);
        assert_eq!(valid_idempotency_key("1,2"), None);
        assert_eq!(valid_idempotency_key("+1"), None);
    }

    #[test]
    fn public_command_parser_accepts_only_participant_verbs() {
        assert!(matches!(
            parse_public_command("{\"command\":\"activate\"}"),
            CommandParse::Public(SessionCommand::Activate)
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"pause\"}"),
            CommandParse::Public(SessionCommand::Pause)
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"resume\"}"),
            CommandParse::Public(SessionCommand::Resume)
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"complete\"}"),
            CommandParse::Public(SessionCommand::Complete)
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"cancel\"}"),
            CommandParse::Public(SessionCommand::Cancel)
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"begin_scoring\"}"),
            CommandParse::ScoringOrOperator
        ));
        for operator in ["record_score", "release", "expire", "invalidate"] {
            assert!(matches!(
                parse_public_command(&format!("{{\"command\":\"{operator}\"}}")),
                CommandParse::ScoringOrOperator
            ));
        }
        assert!(matches!(
            parse_public_command("{\"command\":\"nope\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(parse_public_command("{}"), CommandParse::Invalid));
        assert!(matches!(
            parse_public_command("{\"command\":\"activate\",\"extra\":\"x\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"activate\",\"command\":\"pause\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"act\\nivate\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"a\\q\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"\u{0001}\"}"),
            CommandParse::Invalid
        ));
        assert!(matches!(
            parse_public_command("{\"command\":\"activate\",}"),
            CommandParse::Invalid
        ));
    }

    #[test]
    fn next_actions_cover_every_lifecycle_state() {
        assert_eq!(
            illegal_transition_next_action(SessionState::Created, SessionCommand::Pause),
            "POST activate before pause or resume"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Created, SessionCommand::Resume),
            "POST activate before pause or resume"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Created, SessionCommand::Complete),
            "POST activate, record every published item, then Complete"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Paused, SessionCommand::Complete),
            "POST resume, finish remaining items, then Complete"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Completed, SessionCommand::Activate),
            "This session is already past response collection. Use GET /v1/results/{result_ref} after scoring releases a snapshot"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Scoring, SessionCommand::Activate),
            "This session is already past response collection. Use GET /v1/results/{result_ref} after scoring releases a snapshot"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Scored, SessionCommand::Activate),
            "This session is already past response collection. Use GET /v1/results/{result_ref} after scoring releases a snapshot"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Cancelled, SessionCommand::Activate),
            "This session cannot accept more commands. Start a new session from a published release"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Expired, SessionCommand::Activate),
            "This session cannot accept more commands. Start a new session from a published release"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Invalidated, SessionCommand::Activate),
            "This session cannot accept more commands. Start a new session from a published release"
        );
        assert_eq!(
            illegal_transition_next_action(SessionState::Active, SessionCommand::Expire),
            "Use a command that is legal from the current session state"
        );
        assert_eq!(
            next_action(SessionState::Active),
            "POST /v1/sessions/{session_ref}/responses for each published item, or POST pause to take a break"
        );
        assert_eq!(
            next_action(SessionState::Paused),
            "POST resume when the participant is ready, then continue responses"
        );
        assert_eq!(
            next_action(SessionState::Completed),
            "Do not post more responses. Wait for scoring, then GET /v1/results/{result_ref}"
        );
        assert_eq!(
            next_action(SessionState::Cancelled),
            "Do not post responses. Start a new session if the participant should try again"
        );
        assert_eq!(
            next_action(SessionState::Created),
            "POST activate to begin the assessment"
        );
        assert_eq!(
            next_action(SessionState::Scoring),
            "Use GET /v1/results/{result_ref} after the scored snapshot is released"
        );
        assert_eq!(
            next_action(SessionState::Scored),
            "Use GET /v1/results/{result_ref} after the scored snapshot is released"
        );
        assert_eq!(
            next_action(SessionState::Released),
            "Use GET /v1/results/{result_ref} after the scored snapshot is released"
        );
        assert_eq!(
            next_action(SessionState::Expired),
            "Start a new session from a published release"
        );
        assert_eq!(
            next_action(SessionState::Invalidated),
            "Start a new session from a published release"
        );
    }

    #[test]
    fn request_read_and_transition_problems_fail_closed() {
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        assert!(matches!(
            apply_request_read(&mut Vec::new(), b"POST", Ok(4)).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES];
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
        let invalid_ref = transition_problem(TransitionError::new(
            SessionState::Created,
            SessionCommand::Activate,
            TransitionErrorKind::InvalidReference,
        ));
        assert_eq!(invalid_ref.status(), 400);
        let invalid_seq = transition_problem(TransitionError::new(
            SessionState::Created,
            SessionCommand::Activate,
            TransitionErrorKind::InvalidSequence,
        ));
        assert_eq!(invalid_seq.status(), 409);
        assert!(invalid_seq.body().contains("next command sequence"));
    }
}
