//! Hardened public HTTP boundary for item-delivery evidence.
//!
//! The underlying item-delivery handler remains intentionally small. This module
//! owns the transport-facing authority and HTTP/1.1 framing boundary: callers seed
//! an authoritative [`AssessmentSession`] together with the ledger bound to the same
//! immutable release, lifecycle state is read from that aggregate immediately before
//! every request, and ambiguous request framing is rejected before application code
//! runs. Clients cannot set an arbitrary transport-local session state.

#[path = "item_delivery_http.rs"]
mod implementation;

pub use implementation::{
    ItemDeliveryHttpResponse, ITEM_DELIVERY_COLLECTION_SUFFIX, ITEM_DELIVERY_HTTP_IO_TIMEOUT,
    ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES,
};

use crate::item_delivery::ItemDeliveryLedger;
use crate::session::AssessmentSession;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const HTTP_FIELD_NAME_BYTES: &[u8] =
    b"!#$%&'*+-.0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ^_`abcdefghijklmnopqrstuvwxyz|~";

/// Fail-closed error returned when authoritative sessions and delivery ledgers cannot be paired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ItemDeliveryHttpRuntimeSeedError {
    /// Two seed entries claimed the same opaque assessment-session reference.
    DuplicateSession,
    /// The session and ledger disagree on session, release, digest, or locale provenance.
    SessionLedgerMismatch,
}

impl Display for ItemDeliveryHttpRuntimeSeedError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateSession => {
                "item-delivery HTTP runtime cannot seed the same session reference twice"
            }
            Self::SessionLedgerMismatch => {
                "item-delivery HTTP session and ledger must bind the same session, release, digest, and locale"
            }
        })
    }
}

impl Error for ItemDeliveryHttpRuntimeSeedError {}

/// In-process item-delivery runtime backed by server-authoritative assessment sessions.
///
/// Each ledger is paired once with the [`AssessmentSession`] that owns its lifecycle.
/// The transport never exposes a setter for lifecycle state. Product code advances a
/// session only through the aggregate's command API, and this boundary copies the
/// aggregate's current state into the private delivery handler immediately before a
/// request is evaluated.
pub struct ItemDeliveryHttpRuntime {
    sessions: HashMap<String, AssessmentSession>,
    delivery: implementation::ItemDeliveryHttpRuntime,
}

impl ItemDeliveryHttpRuntime {
    /// Seed exact authoritative sessions and their immutable-release delivery ledgers.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryHttpRuntimeSeedError::DuplicateSession`] when two entries
    /// claim one `session_ref`, or [`ItemDeliveryHttpRuntimeSeedError::SessionLedgerMismatch`]
    /// when a ledger is rebound to a different session, release, content digest, or locale.
    pub fn new(
        seeds: Vec<(AssessmentSession, ItemDeliveryLedger)>,
    ) -> Result<Self, ItemDeliveryHttpRuntimeSeedError> {
        let mut sessions = HashMap::with_capacity(seeds.len());
        let mut delivery_seeds = Vec::with_capacity(seeds.len());
        for (session, ledger) in seeds {
            if session.session_ref() != ledger.session_ref()
                || session.instrument_release_ref() != ledger.instrument_release_ref()
                || session.instrument_release_content_digest() != ledger.release_content_digest()
                || session.locale() != ledger.locale()
            {
                return Err(ItemDeliveryHttpRuntimeSeedError::SessionLedgerMismatch);
            }
            let session_ref = session.session_ref().to_owned();
            if sessions.contains_key(&session_ref) {
                return Err(ItemDeliveryHttpRuntimeSeedError::DuplicateSession);
            }
            delivery_seeds.push((session.state(), ledger));
            sessions.insert(session_ref, session);
        }
        Ok(Self {
            sessions,
            delivery: implementation::ItemDeliveryHttpRuntime::new(delivery_seeds),
        })
    }

    /// Borrow one authoritative aggregate so product code can apply lifecycle commands.
    ///
    /// The returned session exposes only the normal [`AssessmentSession`] API; its
    /// lifecycle state remains private and can therefore advance only through legal
    /// server-authoritative commands.
    #[must_use]
    pub fn session_mut(&mut self, session_ref: &str) -> Option<&mut AssessmentSession> {
        self.sessions.get_mut(session_ref)
    }

    /// Return how many immutable item-delivery events one session currently holds.
    #[must_use]
    pub fn event_count(&self, session_ref: &str) -> usize {
        self.delivery.event_count(session_ref)
    }

    fn synchronize_authoritative_states(&mut self) {
        for (session_ref, session) in &self.sessions {
            self.delivery
                .set_session_state(session_ref, session.state());
        }
    }
}

/// Translate one complete HTTP/1.1 request into an item-delivery response.
///
/// Request text is accepted only when headers are CRLF-delimited and unambiguous,
/// `Transfer-Encoding` is absent, at most one `Content-Length` and one semantic
/// `Idempotency-Key` are present, and the declared body length exactly equals the
/// UTF-8 request body. Invalid framing returns a normal 400 problem response rather
/// than reaching string slicing or item-delivery mutation.
#[must_use]
pub fn handle_item_delivery_http_request(
    request: &str,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    runtime.synchronize_authoritative_states();
    if validate_complete_request(request).is_err() {
        return implementation::handle_item_delivery_http_request("", &mut runtime.delivery);
    }
    implementation::handle_item_delivery_http_request(request, &mut runtime.delivery)
}

/// Bind a blocking TCP listener for public item-delivery HTTP.
///
/// # Errors
///
/// Returns the operating-system error when the address cannot be bound.
pub fn bind_item_delivery_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve one fully framed item-delivery request.
///
/// The listener uses one overall request deadline rather than refreshing a timeout
/// after every fragment. It rejects non-UTF-8 headers, lone-LF header lines,
/// `Transfer-Encoding`, duplicate framing or idempotency headers, malformed or
/// oversized `Content-Length`, incomplete bodies, and bytes trailing the declared
/// request frame before the product handler runs.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] for malformed or ambiguous framing,
/// [`io::ErrorKind::TimedOut`] when the overall request deadline expires, or the
/// underlying socket error for accept/read/write failures.
pub fn accept_one_item_delivery_http(
    listener: &TcpListener,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    let deadline = Instant::now() + ITEM_DELIVERY_HTTP_IO_TIMEOUT;
    stream.set_write_timeout(Some(ITEM_DELIVERY_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream, deadline)?;
    let response = handle_item_delivery_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn validate_complete_request(request: &str) -> io::Result<()> {
    let Some(header_offset) = request
        .as_bytes()
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
    else {
        return Err(incomplete_request_error());
    };
    let body_start = header_offset + 4;
    reject_non_crlf_header_lines(&request.as_bytes()[..body_start])?;
    let headers = &request[..body_start];
    reject_transfer_encoding(headers)?;
    let _ = single_header_value(headers, "idempotency-key")?;
    let expected = match single_header_value(headers, "content-length")? {
        Some(value) => declared_request_end(body_start, value)?,
        None => body_start,
    };
    if expected != request.len() || request.get(body_start..expected).is_none() {
        return Err(incomplete_request_error());
    }
    Ok(())
}

fn read_http_request(stream: &mut TcpStream, deadline: Instant) -> io::Result<String> {
    let mut buffer = vec![0_u8; ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES];
    let mut filled = 0;
    loop {
        if filled == buffer.len() {
            return Err(request_size_error());
        }
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
        if expected > buffer.len() {
            return Err(request_size_error());
        }
        if filled < expected {
            continue;
        }
        if filled > expected {
            return Err(trailing_request_bytes_error());
        }
        return String::from_utf8(buffer[..filled].to_vec())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }
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

fn reject_non_crlf_header_lines(header_bytes: &[u8]) -> io::Result<()> {
    let mut index = 0;
    while index < header_bytes.len() {
        match header_bytes[index] {
            b'\r' => {
                if header_bytes.get(index + 1) != Some(&b'\n') {
                    return Err(malformed_header_error());
                }
                index += 2;
            }
            b'\n' => return Err(malformed_header_error()),
            _ => index += 1,
        }
    }
    Ok(())
}

fn single_header_value<'a>(headers: &'a str, name: &str) -> io::Result<Option<&'a str>> {
    let mut found = None;
    for line in headers.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        let Some((header_name, value)) = line.split_once(':') else {
            return Err(malformed_header_error());
        };
        reject_invalid_header_name(header_name)?;
        if !header_name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "item-delivery HTTP request contains duplicate semantic headers",
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
        Err(malformed_header_error())
    } else {
        Ok(())
    }
}

fn reject_transfer_encoding(headers: &str) -> io::Result<()> {
    if single_header_value(headers, "transfer-encoding")?.is_some() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "item-delivery HTTP listener does not support Transfer-Encoding",
        ))
    } else {
        Ok(())
    }
}

fn declared_request_end(body_start: usize, value: &str) -> io::Result<usize> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_content_length_error());
    }
    let length = value
        .parse::<usize>()
        .map_err(|_| invalid_content_length_error())?;
    body_start
        .checked_add(length)
        .ok_or_else(request_size_error)
}

fn malformed_header_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "item-delivery HTTP request contains a malformed header field",
    )
}

fn invalid_content_length_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid item-delivery HTTP Content-Length",
    )
}

fn incomplete_request_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "item-delivery HTTP request ended before one complete frame was received",
    )
}

fn trailing_request_bytes_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "item-delivery HTTP request contains bytes beyond one framed request",
    )
}

fn request_size_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "item-delivery HTTP request exceeded the accepted size",
    )
}

fn request_deadline_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "item-delivery HTTP request exceeded the overall read deadline",
    )
}

fn write_http_response(
    stream: &mut impl Write,
    response: &ItemDeliveryHttpResponse,
) -> io::Result<()> {
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
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)
}

const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        declared_request_end, reject_invalid_header_name, reject_non_crlf_header_lines,
        reject_transfer_encoding, single_header_value, validate_complete_request,
    };
    use std::io;

    #[test]
    fn framing_helpers_reject_ambiguous_headers_and_trailing_bytes() {
        let duplicate = "POST / HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(
            validate_complete_request(duplicate).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let transfer = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            reject_transfer_encoding(transfer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let duplicate_key = "POST / HTTP/1.1\r\nIdempotency-Key: a\r\nIdempotency-Key: a\r\n\r\n";
        assert_eq!(
            single_header_value(duplicate_key, "idempotency-key")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let trailing = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\nx";
        assert_eq!(
            validate_complete_request(trailing).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn header_and_length_helpers_use_exact_http_grammar() {
        assert!(reject_non_crlf_header_lines(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").is_ok());
        assert_eq!(
            reject_non_crlf_header_lines(b"GET / HTTP/1.1\nHost: x\r\n\r\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(reject_invalid_header_name("Content-Length").is_ok());
        assert!(reject_invalid_header_name("Content Length").is_err());
        assert_eq!(declared_request_end(10, "2").unwrap(), 12);
        assert!(declared_request_end(10, "+2").is_err());
    }
}
