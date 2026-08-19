//! Hardened public HTTP/1.1 framing boundary for assessment sessions.
//!
//! The domain transport implementation remains in `session_http.rs`; this
//! facade keeps its public API while making message framing a fail-closed
//! boundary. The listener supports exactly one optional `Content-Length`
//! header and no `Transfer-Encoding`. Rejecting unsupported or ambiguous
//! framing prevents intermediaries and this server from disagreeing about
//! where a request ends.

#[expect(
    dead_code,
    reason = "the private legacy listener is shadowed while its domain transport implementation is reused"
)]
#[path = "session_http.rs"]
mod implementation;

pub use implementation::{
    bind_session_http, handle_session_http_request, MemorySessionHttpPort, PostgresSessionHttpPort,
    SessionHttpPort, SessionHttpResponse, SESSION_COLLECTION_PATH, SESSION_HTTP_IO_TIMEOUT,
    SESSION_HTTP_MAX_REQUEST_BYTES,
};

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

/// Accept one TCP connection and serve a single persist-backed session request.
///
/// HTTP/1.1 request framing is deliberately narrower than the protocol's full
/// grammar: this listener accepts no `Transfer-Encoding` and at most one
/// `Content-Length`. Unsupported, duplicate, or malformed header fields fail
/// closed before application dispatch, so a proxy and the runtime cannot
/// select different request boundaries.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] for malformed, ambiguous, or
/// unsupported message framing, or the underlying I/O error if accept, read,
/// or write fails.
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
        reject_transfer_encoding(headers)?;
        if let Some(value) = single_header_value(headers, "content-length")? {
            let expected = declared_request_end(body_start, value)?;
            reject_oversized_request(expected, buffer.len())?;
            if filled < expected {
                continue;
            }
            filled = expected;
        }
        break;
    }
    String::from_utf8(buffer[..filled].to_vec())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn single_header_value<'a>(headers: &'a str, name: &str) -> io::Result<Option<&'a str>> {
    let mut found = None;
    for line in headers.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((header_name, value)) = line.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session HTTP request contains a malformed header field",
            ));
        };
        if !header_name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session HTTP request contains duplicate framing headers",
            ));
        }
        found = Some(value.trim());
    }
    Ok(found)
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
        declared_request_end, reject_full_request_buffer, reject_oversized_request,
        reject_transfer_encoding, single_header_value,
    };

    #[test]
    fn framing_helpers_fail_closed_on_duplicate_unsupported_or_malformed_headers() {
        let plain = "POST /v1/sessions HTTP/1.1\r\nHost: example.test\r\n\r\n";
        assert_eq!(single_header_value(plain, "content-length").unwrap(), None);
        assert!(reject_transfer_encoding(plain).is_ok());

        let one = "POST /v1/sessions HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
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
            std::io::ErrorKind::InvalidData
        );

        let malformed = "POST /v1/sessions HTTP/1.1\r\nBroken Header\r\n\r\n";
        assert_eq!(
            single_header_value(malformed, "content-length")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );

        let transfer = "POST /v1/sessions HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            reject_transfer_encoding(transfer).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn framing_size_helpers_cover_valid_invalid_and_overflow_paths() {
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
        assert!(reject_full_request_buffer(100, 8_192).is_ok());
        assert_eq!(
            reject_full_request_buffer(8_192, 8_192).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(reject_oversized_request(100, 8_192).is_ok());
        assert_eq!(
            reject_oversized_request(20_000, 8_192).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
