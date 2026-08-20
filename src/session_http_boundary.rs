//! Hardened public HTTP/1.1 request-boundary checks for assessment sessions.
//!
//! `session_http.rs` contains the session transport behavior. This module keeps
//! the same public API while validating request framing and request identity:
//! the rules that decide where one HTTP request ends and which idempotency key
//! names one session create. It accepts exactly one optional `Content-Length`,
//! no `Transfer-Encoding`, and at most one `Idempotency-Key` field. Invalid or
//! ambiguous requests are rejected before application code runs (a fail-closed
//! policy). That prevents proxies, gateways, and other HTTP intermediaries from
//! choosing a different request boundary or replay identity than this server.

#[path = "session_http.rs"]
mod implementation;

pub use implementation::{
    bind_session_http, MemorySessionHttpPort, PostgresSessionHttpPort, SessionHttpPort,
    SESSION_COLLECTION_PATH, SESSION_HTTP_IO_TIMEOUT, SESSION_HTTP_MAX_REQUEST_BYTES,
};

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const HTTP_FIELD_NAME_BYTES: &[u8] =
    b"!#$%&'*+-.0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ^_`abcdefghijklmnopqrstuvwxyz|~";

/// HTTP response produced by a public session request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl SessionHttpResponse {
    fn from_implementation(response: implementation::SessionHttpResponse) -> Self {
        Self {
            status: response.status(),
            content_type: response.content_type(),
            body: response.body().to_owned(),
        }
    }

    fn duplicate_idempotency_key() -> Self {
        Self {
            status: 400,
            content_type: "application/problem+json",
            body: String::from(
                "{\"type\":\"urn:psychometrics-commons:problem:invalid-idempotency-key\",\"title\":\"Invalid Idempotency Key\",\"status\":400,\"detail\":\"POST /v1/sessions accepts exactly one Idempotency-Key header\"}",
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
///
/// A repeated `Idempotency-Key` is rejected before session lookup or mutation,
/// including equal duplicates. HTTP permits repeated field lines only when a
/// field's semantics define how they combine; session-create idempotency is one
/// opaque identity, not a list.
#[must_use]
pub fn handle_session_http_request<P: SessionHttpPort>(
    request: &str,
    port: &mut P,
    created_at_unix_ms: u64,
) -> SessionHttpResponse {
    if has_duplicate_header(request, "idempotency-key") {
        return SessionHttpResponse::duplicate_idempotency_key();
    }
    SessionHttpResponse::from_implementation(implementation::handle_session_http_request(
        request,
        port,
        created_at_unix_ms,
    ))
}

/// Accept one TCP connection and serve one persist-backed session request.
///
/// Request framing means deciding exactly which bytes belong to this request.
/// This deliberately small HTTP/1.1 listener accepts no `Transfer-Encoding`,
/// at most one `Content-Length`, and at most one `Idempotency-Key`. Malformed
/// or ambiguous headers are rejected before session handling begins. The
/// connection also has one overall read deadline, so sending tiny fragments
/// cannot keep a worker occupied forever.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] when request boundaries or headers are
/// malformed or ambiguous, [`io::ErrorKind::TimedOut`] when the overall request
/// deadline expires, or the underlying I/O error when accept, read, or write
/// fails.
pub fn accept_one_session_http<P: SessionHttpPort>(
    listener: &TcpListener,
    port: &mut P,
    created_at_unix_ms: u64,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    let deadline = Instant::now() + SESSION_HTTP_IO_TIMEOUT;
    stream.set_write_timeout(Some(SESSION_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream, deadline)?;
    let response = handle_session_http_request(&request, port, created_at_unix_ms);
    write_http_response(&mut stream, &response)
}

fn read_http_request(stream: &mut TcpStream, deadline: Instant) -> io::Result<String> {
    let mut buffer = vec![0_u8; SESSION_HTTP_MAX_REQUEST_BYTES];
    let mut filled = 0;
    loop {
        reject_full_request_buffer(filled, buffer.len())?;
        stream.set_read_timeout(Some(remaining_request_timeout(deadline)?))?;
        let read = stream
            .read(&mut buffer[filled..])
            .map_err(normalize_read_error)?;
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
        reject_non_crlf_header_lines(&buffer[..body_start])?;
        let headers = std::str::from_utf8(&buffer[..body_start])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        reject_transfer_encoding(headers)?;
        let _ = single_header_value(headers, "idempotency-key")?;
        let expected = match single_header_value(headers, "content-length")? {
            Some(value) => declared_request_end(body_start, value)?,
            None => body_start,
        };
        reject_oversized_request(expected, buffer.len())?;
        if filled < expected {
            continue;
        }
        if filled > expected {
            return Err(trailing_request_bytes_error());
        }
        filled = expected;
        break;
    }
    String::from_utf8(buffer[..filled].to_vec())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn remaining_request_timeout(deadline: Instant) -> io::Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(request_deadline_error())
    } else {
        Ok(remaining)
    }
}

fn normalize_read_error(error: io::Error) -> io::Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        request_deadline_error()
    } else {
        error
    }
}

fn request_deadline_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "session HTTP request exceeded the overall read deadline",
    )
}

fn trailing_request_bytes_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "session HTTP request contains bytes beyond one framed request",
    )
}

fn reject_non_crlf_header_lines(header_bytes: &[u8]) -> io::Result<()> {
    let mut index = 0;
    while index < header_bytes.len() {
        match header_bytes[index] {
            b'\r' => {
                if header_bytes.get(index + 1) != Some(&b'\n') {
                    return Err(malformed_header_field_error());
                }
                index += 2;
            }
            b'\n' => return Err(malformed_header_field_error()),
            _ => index += 1,
        }
    }
    Ok(())
}

fn has_duplicate_header(request: &str, name: &str) -> bool {
    let mut found = false;
    for line in request.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((header_name, _)) = line.split_once(':') else {
            continue;
        };
        if !header_name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found {
            return true;
        }
        found = true;
    }
    false
}

fn single_header_value<'a>(headers: &'a str, name: &str) -> io::Result<Option<&'a str>> {
    let mut found = None;
    for line in headers.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((header_name, value)) = line.split_once(':') else {
            return Err(malformed_header_field_error());
        };
        reject_invalid_header_name(header_name)?;
        if !header_name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session HTTP request contains duplicate singleton headers",
            ));
        }
        found = Some(value.trim_matches(&[' ', '\t'][..]));
    }
    Ok(found)
}

fn reject_invalid_header_name(header_name: &str) -> io::Result<()> {
    if header_name.is_empty()
        || !header_name
            .bytes()
            .all(|byte| HTTP_FIELD_NAME_BYTES.contains(&byte))
    {
        Err(malformed_header_field_error())
    } else {
        Ok(())
    }
}

fn malformed_header_field_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "session HTTP request contains a malformed header field",
    )
}

fn reject_transfer_encoding(headers: &str) -> io::Result<()> {
    if single_header_value(headers, "transfer-encoding")?.is_some() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session HTTP listener does not support Transfer-Encoding",
        ))
    } else {
        Ok(())
    }
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

fn invalid_content_length_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid session HTTP Content-Length",
    )
}

fn declared_request_end(body_start: usize, content_length: &str) -> io::Result<usize> {
    if content_length.is_empty() || !content_length.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_content_length_error());
    }
    let content_length = content_length
        .parse::<usize>()
        .map_err(|_| invalid_content_length_error())?;
    body_start.checked_add(content_length).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "session HTTP request exceeded the accepted size",
        )
    })
}

fn write_http_response(stream: &mut impl Write, response: &SessionHttpResponse) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        status = response.status(),
        reason = reason_phrase(response.status()),
        content_type = response.content_type(),
        len = response.body().len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(response.body().as_bytes())?;
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
        declared_request_end, has_duplicate_header, normalize_read_error, reason_phrase,
        reject_full_request_buffer, reject_invalid_header_name, reject_non_crlf_header_lines,
        reject_oversized_request, reject_transfer_encoding, remaining_request_timeout,
        single_header_value,
    };
    use std::io;
    use std::time::{Duration, Instant};

    #[test]
    fn framing_helpers_fail_closed_on_duplicate_unsupported_or_malformed_headers() {
        let plain = "POST /v1/sessions HTTP/1.1\r\nHost: example.test\r\n\r\n";
        assert_eq!(single_header_value(plain, "content-length").unwrap(), None);
        assert!(reject_transfer_encoding(plain).is_ok());

        let one = "POST /v1/sessions HTTP/1.1\r\nContent-Length: \t2 \t\r\n\r\n{}";
        assert_eq!(
            single_header_value(one, "content-length").unwrap(),
            Some("2")
        );

        let duplicate =
            "POST /v1/sessions HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(
            single_header_value(duplicate, "content-length")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        let duplicate_idempotency = "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: ses_a\r\nIdempotency-Key: ses_b\r\n\r\n";
        assert!(has_duplicate_header(
            duplicate_idempotency,
            "idempotency-key"
        ));
        assert_eq!(
            single_header_value(duplicate_idempotency, "idempotency-key")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(!has_duplicate_header(one, "idempotency-key"));
        assert!(!has_duplicate_header(
            "POST /v1/sessions HTTP/1.1\r\nBroken Header\r\nIdempotency-Key: ses_a\r\n\r\n",
            "idempotency-key"
        ));

        let malformed = "POST /v1/sessions HTTP/1.1\r\nBroken Header\r\n\r\n";
        assert_eq!(
            single_header_value(malformed, "content-length")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        let transfer = "POST /v1/sessions HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            reject_transfer_encoding(transfer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn response_reason_phrases_cover_public_and_fallback_statuses() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(418), "Error");
    }

    #[test]
    fn request_deadline_helpers_cover_expiry_and_socket_timeout_kinds() {
        assert!(remaining_request_timeout(Instant::now() + Duration::from_secs(1)).is_ok());
        assert_eq!(
            remaining_request_timeout(Instant::now())
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::new(io::ErrorKind::TimedOut, "timeout")).kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::new(io::ErrorKind::WouldBlock, "would block")).kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::new(io::ErrorKind::ConnectionReset, "reset")).kind(),
            io::ErrorKind::ConnectionReset
        );
    }

    #[test]
    fn header_lines_require_crlf_delimiters() {
        assert!(
            reject_non_crlf_header_lines(b"GET / HTTP/1.1\r\nHost: example.test\r\n\r\n").is_ok()
        );
        assert_eq!(
            reject_non_crlf_header_lines(b"GET / HTTP/1.1\nHost: example.test\r\n\r\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            reject_non_crlf_header_lines(b"GET / HTTP/1.1\rHost: example.test\r\n\r\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn header_names_require_exact_http_token_grammar() {
        assert!(reject_invalid_header_name("Content-Length").is_ok());
        assert_eq!(
            reject_invalid_header_name("").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            reject_invalid_header_name("Content-Length ")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            reject_invalid_header_name("Content@Length")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn framing_size_helpers_cover_valid_invalid_and_overflow_paths() {
        assert_eq!(declared_request_end(32, "8").unwrap(), 40);
        assert_eq!(
            declared_request_end(32, "").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(32, "no").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(32, "+8").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(32, "18446744073709551615")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_full_request_buffer(100, 8_192).is_ok());
        assert_eq!(
            reject_full_request_buffer(8_192, 8_192).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_oversized_request(100, 8_192).is_ok());
        assert_eq!(
            reject_oversized_request(20_000, 8_192).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
