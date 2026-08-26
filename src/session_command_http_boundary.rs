//! Hardened HTTP/1.1 framing boundary for participant session commands.
//!
//! The command implementation owns application semantics. This boundary waits
//! for the declared body, rejects ambiguous framing and duplicate client-command
//! identities, and only then dispatches the request. That keeps intermediaries
//! and this process from disagreeing about request or idempotency boundaries.

#[allow(dead_code)]
#[path = "session_command_http.rs"]
mod implementation;

pub use implementation::{
    bind_session_command_http, handle_session_command_http_request, SessionCommandHttpResponse,
    SessionCommandHttpRuntime, SESSION_COMMAND_HTTP_IO_TIMEOUT,
    SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES,
};

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const HTTP_FIELD_NAME_BYTES: &[u8] =
    b"!#$%&'*+-.0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ^_`abcdefghijklmnopqrstuvwxyz|~";

/// Accept one TCP connection and serve one fully framed session-command request.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] for malformed, ambiguous, oversized,
/// incomplete, or multi-request framing; [`io::ErrorKind::TimedOut`] when the
/// overall request deadline expires; or the underlying accept/read/write error.
pub fn accept_one_session_command_http(
    listener: &TcpListener,
    runtime: &mut SessionCommandHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    let deadline = Instant::now() + SESSION_COMMAND_HTTP_IO_TIMEOUT;
    stream.set_write_timeout(Some(SESSION_COMMAND_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream, deadline)?;
    let response = handle_session_command_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn read_http_request(stream: &mut TcpStream, deadline: Instant) -> io::Result<String> {
    let mut buffer = vec![0_u8; SESSION_COMMAND_HTTP_MAX_REQUEST_BYTES];
    let mut filled = 0;
    loop {
        reject_full_request_buffer(filled, buffer.len())?;
        stream.set_read_timeout(Some(remaining_request_timeout(deadline)?))?;
        let read = stream
            .read(&mut buffer[filled..])
            .map_err(normalize_read_error)?;
        if read == 0 {
            return Err(incomplete_request_error());
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
        "session-command HTTP request exceeded the overall read deadline",
    )
}

fn incomplete_request_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "session-command HTTP request ended before one complete frame was received",
    )
}

fn trailing_request_bytes_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "session-command HTTP request contains bytes beyond one framed request",
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
                "session-command HTTP request contains a duplicate singleton header",
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
        "session-command HTTP request contains a malformed header field",
    )
}

fn reject_transfer_encoding(headers: &str) -> io::Result<()> {
    if single_header_value(headers, "transfer-encoding")?.is_some() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session-command HTTP listener does not support Transfer-Encoding",
        ))
    } else {
        Ok(())
    }
}

fn reject_full_request_buffer(filled: usize, capacity: usize) -> io::Result<()> {
    if filled == capacity {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session-command HTTP request exceeded the accepted size",
        ))
    } else {
        Ok(())
    }
}

fn reject_oversized_request(expected: usize, capacity: usize) -> io::Result<()> {
    if expected > capacity {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session-command HTTP request exceeded the accepted size",
        ))
    } else {
        Ok(())
    }
}

fn invalid_content_length_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid session-command HTTP Content-Length",
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
            "session-command HTTP request exceeded the accepted size",
        )
    })
}

fn write_http_response(
    stream: &mut impl Write,
    response: &SessionCommandHttpResponse,
) -> io::Result<()> {
    let allow = if response.status() == 405 {
        "Allow: POST\r\n"
    } else {
        ""
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nCache-Control: no-store\r\n{allow}Connection: close\r\n\r\n",
        status = response.status(),
        reason = reason_phrase(response.status()),
        content_type = response.content_type(),
        len = response.body().len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(response.body().as_bytes())
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

#[cfg(test)]
mod tests {
    use super::{
        declared_request_end, normalize_read_error, reason_phrase, reject_full_request_buffer,
        reject_invalid_header_name, reject_non_crlf_header_lines, reject_oversized_request,
        reject_transfer_encoding, remaining_request_timeout, single_header_value,
    };
    use std::io;
    use std::time::{Duration, Instant};

    #[test]
    fn singleton_and_header_syntax_helpers_fail_closed() {
        let plain = "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nHost: example.test\r\n\r\n";
        assert_eq!(single_header_value(plain, "content-length").unwrap(), None);
        assert!(reject_transfer_encoding(plain).is_ok());

        let one = "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nContent-Length: \t2 \t\r\n\r\n{}";
        assert_eq!(
            single_header_value(one, "content-length").unwrap(),
            Some("2")
        );

        for duplicate in [
            "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nIdempotency-Key: idem_one\r\nIdempotency-Key: idem_two\r\n\r\n",
        ] {
            let name = if duplicate.contains("Content-Length") {
                "content-length"
            } else {
                "idempotency-key"
            };
            assert_eq!(
                single_header_value(duplicate, name).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }

        let malformed = "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nBroken Header\r\n\r\n";
        assert_eq!(
            single_header_value(malformed, "content-length")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let transfer = "POST /v1/sessions/ses_one/commands HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            reject_transfer_encoding(transfer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_invalid_header_name("Content-Length").is_ok());
        assert_eq!(
            reject_invalid_header_name("").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            reject_invalid_header_name("Content Length")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_non_crlf_header_lines(b"A: b\r\n\r\n").is_ok());
        assert_eq!(
            reject_non_crlf_header_lines(b"A: b\n\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            reject_non_crlf_header_lines(b"A: b\rX")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn request_bounds_deadline_and_length_helpers_cover_both_sides() {
        assert!(reject_full_request_buffer(1, 2).is_ok());
        assert_eq!(
            reject_full_request_buffer(2, 2).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_oversized_request(2, 2).is_ok());
        assert_eq!(
            reject_oversized_request(3, 2).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(declared_request_end(10, "2").unwrap(), 12);
        assert_eq!(
            declared_request_end(10, "").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(10, "2x").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            declared_request_end(usize::MAX, "1").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(remaining_request_timeout(Instant::now() + Duration::from_secs(1)).is_ok());
        assert_eq!(
            remaining_request_timeout(Instant::now() - Duration::from_millis(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::new(io::ErrorKind::TimedOut, "timeout")).kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::new(io::ErrorKind::WouldBlock, "block")).kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            normalize_read_error(io::Error::other("boom")).kind(),
            io::ErrorKind::Other
        );
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(418), "Error");
    }
}
