//! Operator HTTP probes for process liveness and operation-scoped readiness.
//!
//! These probes translate [`RuntimeHealthSnapshot`] into load-balancer-safe
//! HTTP responses. They do not invent availability SLOs, execute store I/O, or
//! expose raw database or provider errors.

use crate::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, RuntimeHealthSnapshot,
};
use std::fmt::Write;
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Process-liveness probe path.
pub const HEALTH_LIVE_PATH: &str = "/live";
/// Operation-readiness probe path.
pub const HEALTH_READY_PATH: &str = "/ready";
/// Bounded read/write timeout for one accepted probe connection.
pub const HEALTH_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted probe request size, including headers.
pub const HEALTH_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

/// HTTP response produced by a health probe request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl HealthHttpResponse {
    fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
        }
    }

    fn problem(status: u16, title: &str, detail: &str) -> Self {
        Self {
            status,
            content_type: "application/problem+json",
            body: format!(
                "{{\"type\":\"about:blank\",\"title\":{},\"status\":{status},\"detail\":{}}}",
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

/// Translate one raw HTTP/1.1 request into a liveness or readiness response.
///
/// Liveness answers whether the process is live. Readiness answers whether new
/// state-changing work is safe for the caller-named required capabilities.
/// Unknown required capabilities, stalled backlog, unknown integrity, or a
/// non-live process fail closed with HTTP 503. Unsupported methods and paths
/// return RFC 9457 problem details without echoing the raw request.
#[must_use]
pub fn handle_health_http_request(
    request: &str,
    snapshot: &RuntimeHealthSnapshot,
) -> HealthHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return HealthHttpResponse::problem(
            400,
            "Bad Request",
            "health probe request must include an HTTP method and target",
        );
    };
    if method != "GET" {
        return HealthHttpResponse::problem(
            405,
            "Method Not Allowed",
            "health probes accept GET /live and GET /ready only",
        );
    }
    let (path, query) = split_target(target);
    match path {
        HEALTH_LIVE_PATH => {
            let status = if snapshot.is_live() { 200 } else { 503 };
            HealthHttpResponse::json(status, snapshot_body(snapshot, snapshot.is_ready_for(&[])))
        }
        HEALTH_READY_PATH => health_ready_response(snapshot, &required_capabilities(query)),
        _ => HealthHttpResponse::problem(
            404,
            "Not Found",
            "health probes accept GET /live and GET /ready only",
        ),
    }
}

/// Bind a blocking TCP listener for operator health probes.
///
/// The caller chooses the address. Tests and local operators typically bind
/// `127.0.0.1:0`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_health_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single health-probe request.
///
/// The connection is closed after the response. Keep-alive, TLS, and public
/// product routes are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_health_http(
    listener: &TcpListener,
    snapshot: &RuntimeHealthSnapshot,
) -> io::Result<()> {
    accept_one_health_http_with(listener, |request| {
        handle_health_http_request(request, snapshot)
    })
}

/// Accept one TCP connection and answer it with `handler`.
///
/// The handler runs after a bounded read so store observation cannot start
/// before the connection is accepted. Incomplete or oversized requests become
/// empty request text and fail closed as HTTP 400 without echoing input.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_health_http_with<F>(listener: &TcpListener, handler: F) -> io::Result<()>
where
    F: FnOnce(&str) -> HealthHttpResponse,
{
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(HEALTH_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(HEALTH_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handler(&request);
    write_http_response(&mut stream, &response)
}

/// Serve probe requests until `accept` fails.
///
/// Operators run this loop so Kubernetes or a load balancer can keep asking
/// GET `/live` and GET `/ready`. Interrupted accepts retry. Any other accept,
/// read, or write error stops the loop so a closed or non-blocking listener
/// does not spin. TLS, keep-alive, and measured SLO values remain outside
/// this slice.
///
/// # Errors
///
/// Returns the I/O error that stopped the loop.
pub fn serve_health_http(
    listener: &TcpListener,
    snapshot: &RuntimeHealthSnapshot,
) -> io::Result<()> {
    serve_health_http_with(listener, |request| {
        handle_health_http_request(request, snapshot)
    })
}

/// Serve probe requests with `handler` until `accept` fails.
///
/// # Errors
///
/// Returns the I/O error that stopped the loop.
pub fn serve_health_http_with<F>(listener: &TcpListener, mut handler: F) -> io::Result<()>
where
    F: FnMut(&str) -> HealthHttpResponse,
{
    loop {
        match classify_serve_accept(accept_one_health_http_with(listener, |request| {
            handler(request)
        })) {
            ServeAcceptProgress::Continue => {}
            ServeAcceptProgress::Stop(error) => return Err(error),
        }
    }
}

#[derive(Debug)]
enum ServeAcceptProgress {
    Continue,
    Stop(io::Error),
}

fn classify_serve_accept(result: io::Result<()>) -> ServeAcceptProgress {
    match result {
        Ok(()) => ServeAcceptProgress::Continue,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => ServeAcceptProgress::Continue,
        Err(error) => ServeAcceptProgress::Stop(error),
    }
}

/// Return whether this request is GET `/ready` and must observe operational state.
#[must_use]
pub fn health_request_requires_readiness_snapshot(request: &str) -> bool {
    let Some((method, target)) = parse_request_line(request) else {
        return false;
    };
    method == "GET" && split_target(target).0 == HEALTH_READY_PATH
}

/// Return caller-named `capability` query values from one probe request.
#[must_use]
pub fn health_request_required_capabilities(request: &str) -> Vec<&str> {
    parse_request_line(request)
        .map(|(_, target)| required_capabilities(split_target(target).1))
        .unwrap_or_default()
}

/// Answer readiness for caller-named required capabilities.
#[must_use]
pub fn health_ready_response(
    snapshot: &RuntimeHealthSnapshot,
    required_capabilities: &[&str],
) -> HealthHttpResponse {
    let ready = snapshot.is_ready_for(required_capabilities);
    let status = if ready { 200 } else { 503 };
    HealthHttpResponse::json(status, snapshot_body(snapshot, ready))
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
    if buffer.len() > HEALTH_HTTP_MAX_REQUEST_BYTES
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
                || buffer.len() > HEALTH_HTTP_MAX_REQUEST_BYTES
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

fn write_http_response(stream: &mut TcpStream, response: &HealthHttpResponse) -> io::Result<()> {
    let body = response.body().as_bytes();
    let allow = if response.status() == 405 {
        "Allow: GET\r\n"
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
        503 => "Service Unavailable",
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

fn required_capabilities(query: &str) -> Vec<&str> {
    query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == "capability").then_some(value)
        })
        .collect()
}

fn snapshot_body(snapshot: &RuntimeHealthSnapshot, ready: bool) -> String {
    let mut capabilities = String::from("[");
    for (index, capability) in snapshot.capabilities().iter().enumerate() {
        if index > 0 {
            capabilities.push(',');
        }
        capabilities.push_str(&capability_body(capability));
    }
    capabilities.push(']');
    format!(
        "{{\"live\":{},\"ready\":{ready},\"backlog_health\":{},\"data_integrity_health\":{},\"capabilities\":{capabilities}}}",
        json_bool(snapshot.is_live()),
        json_string(backlog_label(snapshot.backlog_health())),
        json_string(integrity_label(snapshot.data_integrity_health())),
    )
}

fn capability_body(capability: &CapabilityHealth) -> String {
    format!(
        "{{\"capability_ref\":{},\"state\":{},\"accepts_new_work\":{}}}",
        json_string(capability.capability_ref()),
        json_string(capability_state_label(capability.state())),
        json_bool(capability.accepts_new_work())
    )
}

const fn backlog_label(health: BacklogHealth) -> &'static str {
    match health {
        BacklogHealth::WithinBounds => "within_bounds",
        BacklogHealth::Stalled => "stalled",
        BacklogHealth::Unknown => "unknown",
    }
}

const fn integrity_label(health: DataIntegrityHealth) -> &'static str {
    match health {
        DataIntegrityHealth::Verified => "verified",
        DataIntegrityHealth::Incompatible => "incompatible",
        DataIntegrityHealth::Unknown => "unknown",
    }
}

const fn capability_state_label(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Available => "available",
        CapabilityState::Degraded => "degraded",
        CapabilityState::Unavailable => "unavailable",
        CapabilityState::Unknown => "unknown",
    }
}

fn json_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
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
        apply_request_read, backlog_label, capability_state_label, classify_serve_accept,
        handle_health_http_request, health_ready_response, health_request_required_capabilities,
        health_request_requires_readiness_snapshot, integrity_label, json_string, reason_phrase,
        RequestReadProgress, ServeAcceptProgress, HEALTH_HTTP_MAX_REQUEST_BYTES, HEALTH_LIVE_PATH,
        HEALTH_READY_PATH,
    };
    use crate::health::{
        BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth,
        RuntimeHealthSnapshot,
    };
    use std::io;

    #[test]
    fn remaining_labels_and_json_escapes_are_stable() {
        assert_eq!(backlog_label(BacklogHealth::Unknown), "unknown");
        assert_eq!(
            integrity_label(DataIntegrityHealth::Incompatible),
            "incompatible"
        );
        assert_eq!(
            capability_state_label(CapabilityState::Degraded),
            "degraded"
        );
        assert_eq!(capability_state_label(CapabilityState::Unknown), "unknown");
        assert_eq!(integrity_label(DataIntegrityHealth::Unknown), "unknown");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(503), "Service Unavailable");
        assert_eq!(reason_phrase(418), "Error");
    }

    #[test]
    fn request_line_rejects_extra_tokens_and_non_http_versions() {
        let snapshot = RuntimeHealthSnapshot::new(
            true,
            BacklogHealth::Unknown,
            DataIntegrityHealth::Incompatible,
            vec![
                CapabilityHealth::new("research_registration", CapabilityState::Degraded, true)
                    .unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(
            handle_health_http_request("GET /live HTTP/1.1 extra\r\n\r\n", &snapshot).status(),
            400
        );
        assert_eq!(
            handle_health_http_request("GET /live SMTP/1.0\r\n\r\n", &snapshot).status(),
            400
        );
        let live = handle_health_http_request(
            &format!("GET {HEALTH_LIVE_PATH}?capability=ignored HTTP/1.1\r\n\r\n"),
            &snapshot,
        );
        assert_eq!(live.status(), 200);
        assert!(live.body().contains("\"backlog_health\":\"unknown\""));
        assert!(live
            .body()
            .contains("\"data_integrity_health\":\"incompatible\""));
        assert!(live.body().contains("\"state\":\"degraded\""));
        assert_eq!(live.content_type(), "application/json");

        let ready_snapshot = RuntimeHealthSnapshot::new(
            true,
            BacklogHealth::WithinBounds,
            DataIntegrityHealth::Verified,
            vec![
                CapabilityHealth::new("research_registration", CapabilityState::Degraded, true)
                    .unwrap(),
            ],
        )
        .unwrap();
        let ready = handle_health_http_request(
            "GET /ready?capability=research_registration HTTP/1.1\r\n\r\n",
            &ready_snapshot,
        );
        assert_eq!(ready.status(), 200);
        assert_eq!(ready.content_type(), "application/json");
        assert!(ready.body().contains("\"ready\":true"));

        let not_allowed = handle_health_http_request("POST /live HTTP/1.1\r\n\r\n", &snapshot);
        assert_eq!(not_allowed.status(), 405);
        assert_eq!(not_allowed.content_type(), "application/problem+json");
        assert!(not_allowed
            .body()
            .contains("\"title\":\"Method Not Allowed\""));

        let missing = handle_health_http_request("GET /v1/sessions HTTP/1.1\r\n\r\n", &snapshot);
        assert_eq!(missing.status(), 404);
        assert_eq!(missing.content_type(), "application/problem+json");
        assert!(missing.body().contains("\"title\":\"Not Found\""));

        let not_live = RuntimeHealthSnapshot::new(
            false,
            BacklogHealth::Stalled,
            DataIntegrityHealth::Unknown,
            vec![
                CapabilityHealth::new("research_registration", CapabilityState::Unavailable, false)
                    .unwrap(),
                CapabilityHealth::new("authenticated_linking", CapabilityState::Available, true)
                    .unwrap(),
            ],
        )
        .unwrap();
        let dead = handle_health_http_request(
            &format!("GET {HEALTH_LIVE_PATH} HTTP/1.1\r\n\r\n"),
            &not_live,
        );
        assert_eq!(dead.status(), 503);
        assert!(dead.body().contains("\"live\":false"));
        assert!(dead.body().contains("\"ready\":false"));
        let not_ready = handle_health_http_request(
            &format!("GET {HEALTH_READY_PATH} HTTP/1.1\r\n\r\n"),
            &not_live,
        );
        assert_eq!(not_ready.status(), 503);
        assert!(not_ready.body().contains("\"ready\":false"));
    }

    #[test]
    fn readiness_helpers_classify_requests_and_required_capabilities() {
        let snapshot = RuntimeHealthSnapshot::new(
            true,
            BacklogHealth::WithinBounds,
            DataIntegrityHealth::Verified,
            vec![
                CapabilityHealth::new("research_registration", CapabilityState::Degraded, true)
                    .unwrap(),
            ],
        )
        .unwrap();
        assert!(!health_request_requires_readiness_snapshot(
            "GET /live HTTP/1.1\r\n\r\n"
        ));
        assert!(!health_request_requires_readiness_snapshot("NOT-A-REQUEST"));
        assert!(!health_request_requires_readiness_snapshot(
            "POST /ready HTTP/1.1\r\n\r\n"
        ));
        assert!(health_request_requires_readiness_snapshot(
            "GET /ready?capability=scoring HTTP/1.1\r\n\r\n"
        ));
        assert!(health_request_required_capabilities("NOT-A-REQUEST").is_empty());
        assert_eq!(
            health_request_required_capabilities(
                "GET /ready?capability=scoring&capability=research_registration HTTP/1.1\r\n\r\n"
            ),
            vec!["scoring", "research_registration"]
        );
        let ready = health_ready_response(&snapshot, &["research_registration"]);
        assert_eq!(ready.status(), 200);
        let unready = health_ready_response(&snapshot, &["unregistered_capability"]);
        assert_eq!(unready.status(), 503);
        assert!(health_request_requires_readiness_snapshot(
            "GET /ready HTTP/1.1\r\n\r\n"
        ));
        assert!(health_request_required_capabilities("GET /ready HTTP/1.1\r\n\r\n").is_empty());
    }

    #[test]
    fn request_read_progress_covers_eof_timeout_and_io_failure() {
        let mut buffer = Vec::new();
        assert!(matches!(
            apply_request_read(&mut buffer, b"", Ok(0)).unwrap(),
            RequestReadProgress::Complete
        ));
        buffer.clear();
        assert!(matches!(
            apply_request_read(&mut buffer, b"GET /l", Ok(6)).unwrap(),
            RequestReadProgress::Continue
        ));
        assert_eq!(buffer, b"GET /l");
        buffer.clear();
        assert!(matches!(
            apply_request_read(&mut buffer, b"GET /live HTTP/1.1\r\n\r\n", Ok(22)).unwrap(),
            RequestReadProgress::Complete
        ));
        buffer.clear();
        let oversized = vec![b'A'; HEALTH_HTTP_MAX_REQUEST_BYTES + 1];
        assert!(matches!(
            apply_request_read(&mut buffer, &oversized, Ok(oversized.len())).unwrap(),
            RequestReadProgress::Complete
        ));
        buffer.clear();
        assert!(matches!(
            apply_request_read(
                &mut buffer,
                b"",
                Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        buffer.clear();
        assert!(matches!(
            apply_request_read(
                &mut buffer,
                b"",
                Err(io::Error::new(io::ErrorKind::WouldBlock, "block"))
            )
            .unwrap(),
            RequestReadProgress::Complete
        ));
        let error = apply_request_read(
            &mut buffer,
            b"",
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset")),
        )
        .expect_err("non-timeout I/O errors must propagate");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
    }

    #[test]
    fn serve_loop_retries_interrupted_accepts_and_stops_on_other_errors() {
        assert!(matches!(
            classify_serve_accept(Ok(())),
            ServeAcceptProgress::Continue
        ));
        assert!(matches!(
            classify_serve_accept(Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "signal"
            ))),
            ServeAcceptProgress::Continue
        ));
        let stopped = classify_serve_accept(Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "closed",
        )));
        match stopped {
            ServeAcceptProgress::Stop(error) => {
                assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
            }
            ServeAcceptProgress::Continue => {
                panic!("non-interrupted accept errors must stop the serve loop")
            }
        }
    }
}
