//! Public HTTP transport for recording and reloading item-delivery evidence.
//!
//! This slice exposes `POST` and `GET`
//! `/v1/sessions/{session_ref}/item-deliveries` over HTTP/1.1. The handler
//! records that one published item version was shown to an Active in-process
//! session. It does not select or calibrate items, persist across restart, or
//! accept responses. Errors use RFC 9457 problem details and never echo raw
//! request bodies, SQL, or provider text.

use crate::item_delivery::{ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest};
use crate::reference::normalized_reference;
use crate::session::SessionState;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter, Write};
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Path suffix under one session for item-delivery evidence.
pub const ITEM_DELIVERY_COLLECTION_SUFFIX: &str = "/item-deliveries";
/// Bounded read/write timeout for one accepted item-delivery HTTP connection.
pub const ITEM_DELIVERY_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted item-delivery HTTP request size, including headers and body.
pub const ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

#[derive(Debug)]
struct DeliverySession {
    state: SessionState,
    ledger: ItemDeliveryLedger,
}

/// Fail-closed error while seeding the in-process item-delivery HTTP runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemDeliveryHttpRuntimeError {
    /// Two seeded ledgers used the same `session_ref`; the last ledger must not win.
    DuplicateSessionRef,
}

impl Display for ItemDeliveryHttpRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateSessionRef => {
                "item-delivery HTTP runtime rejects colliding session_ref seeds"
            }
        })
    }
}

impl Error for ItemDeliveryHttpRuntimeError {}

/// In-process Active-or-later session ledgers the handler may record against.
///
/// This family is a process-local capability-URL: whoever can reach the bound
/// listener and guess a seeded `session_ref` can record or reload deliveries.
/// There is no tenant header and no Keyverse check on this slice. Operators
/// must bind only to a trusted address and seed unique session references.
#[derive(Debug)]
pub struct ItemDeliveryHttpRuntime {
    sessions: HashMap<String, DeliverySession>,
}

impl ItemDeliveryHttpRuntime {
    /// Create a runtime from exact session ledgers the handler may inspect.
    ///
    /// Callers seed the process with already-created sessions and their bound
    /// release manifests. Session HTTP create/start remains a different family.
    /// Colliding `session_ref` values fail closed instead of keeping the last ledger.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryHttpRuntimeError::DuplicateSessionRef`] when two
    /// seeds share one session identity.
    pub fn new(
        sessions: Vec<(SessionState, ItemDeliveryLedger)>,
    ) -> Result<Self, ItemDeliveryHttpRuntimeError> {
        let mut mapped = HashMap::new();
        for (state, ledger) in sessions {
            if mapped
                .insert(
                    ledger.session_ref().to_owned(),
                    DeliverySession { state, ledger },
                )
                .is_some()
            {
                return Err(ItemDeliveryHttpRuntimeError::DuplicateSessionRef);
            }
        }
        Ok(Self { sessions: mapped })
    }

    /// Replace the server-authoritative lifecycle state for one seeded session.
    ///
    /// Returns `false` when the session was never seeded, so callers cannot
    /// treat a missing identity as a successful pause or resume.
    pub fn set_session_state(&mut self, session_ref: &str, state: SessionState) -> bool {
        if let Some(session) = self.sessions.get_mut(session_ref) {
            session.state = state;
            true
        } else {
            false
        }
    }

    /// Return how many accepted deliveries one session currently holds.
    #[must_use]
    pub fn event_count(&self, session_ref: &str) -> usize {
        self.sessions
            .get(session_ref)
            .map_or(0, |session| session.ledger.len())
    }
}

/// HTTP response produced by a public item-delivery request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDeliveryHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl ItemDeliveryHttpResponse {
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

/// Translate one raw HTTP/1.1 request into a delivery record or ledger reload.
///
/// Unknown methods, encoded or numeric session references, missing idempotency
/// keys, items outside the bound release, and inactive sessions fail closed
/// with RFC 9457 problem details. Exact delivery replay returns the original
/// event without inserting a second row.
#[must_use]
pub fn handle_item_delivery_http_request(
    request: &str,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "item delivery request must include an HTTP method and target",
        );
    };
    let path = split_target(target).0;
    let Some(session_ref) = collection_session_ref(path) else {
        if path.starts_with("/v1/sessions/") || path == "/v1/sessions" {
            return method_not_allowed();
        }
        return ItemDeliveryHttpResponse::problem(
            404,
            "urn:psychometrics-commons:problem:not-found",
            "Not Found",
            "item delivery routes accept POST and GET /v1/sessions/{session_ref}/item-deliveries only",
        );
    };
    match method {
        "POST" => handle_record(request, session_ref, runtime),
        "GET" => handle_list(session_ref, runtime),
        _ => method_not_allowed(),
    }
}

/// Bind a blocking TCP listener for public item-delivery HTTP.
///
/// Tests and local operators typically bind `127.0.0.1:0`. Hosted processes bind
/// `0.0.0.0:$PORT`. This function does not start accepting connections.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_item_delivery_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Accept one TCP connection and serve a single item-delivery HTTP request.
///
/// The connection is closed after the response. Keep-alive, TLS, and other
/// public families are outside this slice.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_item_delivery_http(
    listener: &TcpListener,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(ITEM_DELIVERY_HTTP_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(ITEM_DELIVERY_HTTP_IO_TIMEOUT))?;
    let request = read_http_request(&mut stream)?;
    let response = handle_item_delivery_http_request(&request, runtime);
    write_http_response(&mut stream, &response)
}

fn method_not_allowed() -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "item delivery routes accept POST and GET /v1/sessions/{session_ref}/item-deliveries only",
    )
}

fn collection_session_ref(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v1/sessions/")?;
    let (session_ref, suffix) = rest.split_once('/')?;
    (suffix == "item-deliveries"
        && !session_ref.is_empty()
        && !session_ref.contains('/')
        && normalized_reference(session_ref).is_some())
    .then_some(session_ref)
}

fn handle_record(
    request: &str,
    session_ref: &str,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    if !has_json_content_type(request) {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:unsupported-media-type",
            "Unsupported Media Type",
            "POST item delivery requires Content-Type application/json. Set Content-Type to application/json, then retry",
        );
    }
    let Some(idempotency_key) =
        header_value(request, "idempotency-key").and_then(normalized_reference)
    else {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "POST /v1/sessions/{session_ref}/item-deliveries requires an opaque Idempotency-Key header. Set Idempotency-Key to the exact delivery_ref value",
        );
    };
    let Some(body) = request_body(request) else {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "item delivery record requires a JSON object body. Send a complete application/json object whose Content-Length covers the body, then retry",
        );
    };
    let Some(record) = parse_record_body(body) else {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:bad-request",
            "Bad Request",
            "item delivery record requires delivery_ref, item_version_ref, and presentation_context_ref strings",
        );
    };
    if record.delivery_ref != idempotency_key {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:idempotency-mismatch",
            "Idempotency Mismatch",
            "Idempotency-Key must exactly match delivery_ref. Set Idempotency-Key to the exact delivery_ref value",
        );
    }
    let Some(session) = runtime.sessions.get_mut(session_ref) else {
        return session_not_found();
    };
    let previous_len = session.ledger.len();
    let request = ItemDeliveryRequest {
        delivery_ref: &record.delivery_ref,
        item_version_ref: &record.item_version_ref,
        presentation_context_ref: &record.presentation_context_ref,
        selection_evidence_ref: record.selection_evidence_ref.as_deref(),
    };
    match session.ledger.deliver(session.state, request) {
        Ok(event) => {
            let replay = session.ledger.len() == previous_len;
            let status = if replay { 200 } else { 201 };
            ItemDeliveryHttpResponse::json(
                status,
                event_body(&session.ledger, &event, session.state, replay),
            )
        }
        Err(error) => delivery_problem(error),
    }
}

fn handle_list(session_ref: &str, runtime: &ItemDeliveryHttpRuntime) -> ItemDeliveryHttpResponse {
    match runtime.sessions.get(session_ref) {
        Some(session) => ItemDeliveryHttpResponse::json(200, ledger_body(session)),
        None => session_not_found(),
    }
}

fn session_not_found() -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        404,
        "urn:psychometrics-commons:problem:session-not-found",
        "Session Not Found",
        "item delivery requires a session seeded in this process. Create or start this session_ref in this process, then retry",
    )
}

fn delivery_problem(error: ItemDeliveryError) -> ItemDeliveryHttpResponse {
    match error {
        ItemDeliveryError::InvalidReference => ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "item delivery references must be opaque non-numeric values. Replace numeric or blank identifiers with opaque refs, then retry",
        ),
        ItemDeliveryError::SessionNotActive(_) => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:session-not-active",
            "Session Not Active",
            "new item deliveries require an Active assessment session. Return the session to Active, then POST the new delivery. Replay the original Idempotency-Key and body only to recover the accepted event",
        ),
        ItemDeliveryError::IdempotencyConflict => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "delivery_ref was already used for different item-delivery evidence. Replay the original evidence or mint a new delivery_ref that exactly matches Idempotency-Key",
        ),
        ItemDeliveryError::ItemNotInRelease => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:item-not-in-release",
            "Item Not In Release",
            "item version is not part of the bound published instrument release. POST an item_version_ref from the bound release allowed set",
        ),
        ItemDeliveryError::DuplicateItemDelivery => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:duplicate-item-delivery",
            "Duplicate Item Delivery",
            "this item version was already delivered in the session under another delivery_ref. GET the ledger and continue with a different undelivered item",
        ),
    }
}

#[allow(clippy::struct_field_names)]
struct DeliveryRecordBody {
    delivery_ref: String,
    item_version_ref: String,
    presentation_context_ref: String,
    selection_evidence_ref: Option<String>,
}

fn parse_record_body(body: &str) -> Option<DeliveryRecordBody> {
    let fields = parse_string_object(body)?;
    if fields.len() < 3 || fields.len() > 4 {
        return None;
    }
    let selection_evidence_ref = fields.get("selection_evidence_ref").cloned();
    if fields.len() == 4 && selection_evidence_ref.is_none() {
        return None;
    }
    Some(DeliveryRecordBody {
        delivery_ref: fields.get("delivery_ref")?.clone(),
        item_version_ref: fields.get("item_version_ref")?.clone(),
        presentation_context_ref: fields.get("presentation_context_ref")?.clone(),
        selection_evidence_ref,
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

fn event_body(
    ledger: &ItemDeliveryLedger,
    event: &crate::item_delivery::ItemDeliveryEvent,
    state: SessionState,
    replay: bool,
) -> String {
    let selection = match event.selection_evidence_ref() {
        Some(value) => format!(",\"selection_evidence_ref\":{}", json_string(value)),
        None => String::new(),
    };
    format!(
        "{{\"session_ref\":{},\"instrument_release_ref\":{},\"release_content_digest\":{},\"locale\":{},\"session_state\":{},\"delivery_ref\":{},\"item_version_ref\":{},\"presentation_context_ref\":{}{selection},\"sequence\":{},\"next_action\":{}}}",
        json_string(ledger.session_ref()),
        json_string(ledger.instrument_release_ref()),
        json_string(ledger.release_content_digest()),
        json_string(ledger.locale()),
        json_string(session_state_name(state)),
        json_string(event.delivery_ref()),
        json_string(event.item_version_ref()),
        json_string(event.presentation_context_ref()),
        event.sequence(),
        json_string(event_next_action(replay))
    )
}

fn ledger_body(session: &DeliverySession) -> String {
    let mut events = String::from("[");
    for (index, event) in session.ledger.events().iter().enumerate() {
        if index > 0 {
            events.push(',');
        }
        events.push_str(&event_body(&session.ledger, event, session.state, true));
    }
    events.push(']');
    format!(
        "{{\"session_ref\":{},\"instrument_release_ref\":{},\"release_content_digest\":{},\"locale\":{},\"session_state\":{},\"allowed_item_version_refs\":{},\"events\":{events},\"next_action\":{}}}",
        json_string(session.ledger.session_ref()),
        json_string(session.ledger.instrument_release_ref()),
        json_string(session.ledger.release_content_digest()),
        json_string(session.ledger.locale()),
        json_string(session_state_name(session.state)),
        json_string_array(session.ledger.allowed_item_version_refs()),
        json_string(ledger_next_action())
    )
}

fn event_next_action(replay: bool) -> &'static str {
    if replay {
        "Reuse this original event; do not insert another row. Continue from sequence by GET /v1/sessions/{session_ref}/item-deliveries or POST the next undelivered allowed item_version_ref."
    } else {
        "Present item_version_ref from the bound published release using presentation_context_ref. This response is delivery evidence, not item text and not the next selected item. Then POST the next undelivered allowed item or record a response on that family."
    }
}

fn ledger_next_action() -> &'static str {
    "Compare events to allowed_item_version_refs. If session_state is not active, return the session to Active before a new POST. Confirm release_content_digest still matches the published release. Then POST the next undelivered allowed item_version_ref with Idempotency-Key exactly equal to a new delivery_ref."
}

const fn session_state_name(state: SessionState) -> &'static str {
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

fn json_string_array(values: &[String]) -> String {
    let mut encoded = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            encoded.push(',');
        }
        encoded.push_str(&json_string(value));
    }
    encoded.push(']');
    encoded
}

fn has_json_content_type(request: &str) -> bool {
    header_value(request, "content-type").is_some_and(|value| {
        let media_type = value.split(';').next().unwrap_or(value).trim();
        media_type.eq_ignore_ascii_case("application/json")
    })
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
    if body.len() < content_length || !body.is_char_boundary(content_length) {
        return None;
    }
    Some(&body[..content_length])
}

fn header_terminator_index(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn declared_content_length(headers: &str) -> Option<usize> {
    header_value(headers, "content-length")?.parse().ok()
}

fn http_request_complete(buffer: &[u8]) -> bool {
    if buffer.len() > ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES {
        return true;
    }
    let Some(header_end) = header_terminator_index(buffer) else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&buffer[..header_end]) else {
        return true;
    };
    match declared_content_length(headers) {
        Some(content_length) => buffer.len() >= header_end + 4 + content_length,
        None => true,
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
    if buffer.len() > ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES
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
            if http_request_complete(buffer) {
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
        apply_request_read, collection_session_ref, handle_item_delivery_http_request,
        has_json_content_type, http_request_complete, json_string, json_string_array,
        parse_json_string, parse_record_body, parse_string_object, reason_phrase, request_body,
        session_state_name, ItemDeliveryHttpRuntime, ItemDeliveryHttpRuntimeError,
        RequestReadProgress, ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES,
    };
    use crate::instrument::InstrumentReleaseManifest;
    use crate::item_delivery::ItemDeliveryLedger;
    use crate::session::SessionState;
    use std::io::{self, ErrorKind};

    const RELEASE_DIGEST: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const SESSION_REF: &str = "ses_unit_item_delivery";

    fn ledger() -> ItemDeliveryLedger {
        let manifest = InstrumentReleaseManifest::new(
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
            RELEASE_DIGEST,
        )
        .unwrap();
        ItemDeliveryLedger::from_manifest(SESSION_REF, &manifest).unwrap()
    }

    fn runtime() -> ItemDeliveryHttpRuntime {
        ItemDeliveryHttpRuntime::new(vec![(SessionState::Active, ledger())]).unwrap()
    }

    fn post(body: &str, key: &str) -> String {
        format!(
            "POST /v1/sessions/{SESSION_REF}/item-deliveries HTTP/1.1\r\nIdempotency-Key: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn remaining_parse_and_transport_edges_are_stable() {
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(418), "Error");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(json_string_array(&[]), "[]");
        assert_eq!(
            json_string_array(&["item_version_001".to_owned()]),
            "[\"item_version_001\"]"
        );
        assert_eq!(session_state_name(SessionState::Created), "created");
        assert_eq!(session_state_name(SessionState::Active), "active");
        assert_eq!(session_state_name(SessionState::Paused), "paused");
        assert_eq!(session_state_name(SessionState::Completed), "completed");
        assert_eq!(session_state_name(SessionState::Scoring), "scoring");
        assert_eq!(session_state_name(SessionState::Scored), "scored");
        assert_eq!(session_state_name(SessionState::Released), "released");
        assert_eq!(session_state_name(SessionState::Expired), "expired");
        assert_eq!(session_state_name(SessionState::Cancelled), "cancelled");
        assert_eq!(session_state_name(SessionState::Invalidated), "invalidated");
        assert!(!has_json_content_type("POST / HTTP/1.1\r\n\r\n"));
        assert!(has_json_content_type(
            "POST / HTTP/1.1\r\nContent-Type: application/json; charset=utf-8\r\n\r\n"
        ));
        assert_eq!(
            request_body("POST / HTTP/1.1\r\nContent-Length: 1\r\n\r\n한"),
            None
        );
        assert!(!http_request_complete(b"GET / HTTP/1.1\r\n"));
        assert!(http_request_complete(b"GET / HTTP/1.1\r\n\r\n"));
        assert!(!http_request_complete(
            b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nab"
        ));
        assert!(http_request_complete(
            b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nab"
        ));
        assert!(http_request_complete(&[
            0xff, 0xfe, b'\r', b'\n', b'\r', b'\n'
        ]));
        assert_eq!(
            ItemDeliveryHttpRuntimeError::DuplicateSessionRef.to_string(),
            "item-delivery HTTP runtime rejects colliding session_ref seeds"
        );
        let mut runtime = runtime();
        assert!(!runtime.set_session_state("ses_missing", SessionState::Paused));
        assert!(runtime.set_session_state(SESSION_REF, SessionState::Paused));
        assert!(parse_string_object("{").is_none());
        assert!(parse_string_object("{\"a\":\"1\",\"a\":\"2\"}").is_none());
        assert!(parse_string_object("{\"a\":\"1\",}").is_none());
        assert!(parse_record_body("{\"delivery_ref\":\"d\"}").is_none());
        assert!(parse_record_body("{\"delivery_ref\":\"d\",\"item_version_ref\":\"i\",\"presentation_context_ref\":\"p\",\"other\":\"x\"}").is_none());
        assert!(parse_json_string("\"unterminated").is_none());
        assert!(parse_json_string("\"bad\\x\"").is_none());
        assert!(parse_json_string("\"\u{0001}\"").is_none());
        let (decoded, rest) = parse_json_string("\"a\\\"b\\\\c\\n\\r\\t\"tail").unwrap();
        assert_eq!(decoded, "a\"b\\c\n\r\t");
        assert_eq!(rest, "tail");
        assert!(collection_session_ref("/v1/sessions/123/item-deliveries").is_none());
        assert!(collection_session_ref("/v1/sessions/ses_x/item-deliveries/extra").is_none());
        assert_eq!(
            collection_session_ref("/v1/sessions/ses_x/item-deliveries"),
            Some("ses_x")
        );
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
        let mut headers_only = Vec::new();
        let header_chunk = b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\n";
        assert!(matches!(
            apply_request_read(&mut headers_only, header_chunk, Ok(header_chunk.len())).unwrap(),
            RequestReadProgress::Continue
        ));
        let mut oversized = vec![b'x'; ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES];
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
            handle_item_delivery_http_request("NOT-A-REQUEST", &mut runtime).status(),
            400
        );
        assert_eq!(
            handle_item_delivery_http_request("GET /v1/sessions HTTP/1.1\r\n\r\n", &mut runtime)
                .status(),
            405
        );
        assert_eq!(
            handle_item_delivery_http_request(
                "PUT /v1/sessions/ses_x/item-deliveries HTTP/1.1\r\n\r\n",
                &mut runtime
            )
            .status(),
            405
        );
        assert_eq!(
            handle_item_delivery_http_request("GET /v1/results/r1 HTTP/1.1\r\n\r\n", &mut runtime)
                .status(),
            404
        );
        assert_eq!(
            handle_item_delivery_http_request(
                "GET /v1/sessions/ses_missing/item-deliveries HTTP/1.1\r\n\r\n",
                &mut runtime
            )
            .status(),
            404
        );
        assert_eq!(
            handle_item_delivery_http_request(&post("{}", "dlv_ok"), &mut runtime).status(),
            400
        );
        assert_eq!(
            handle_item_delivery_http_request(
                "POST /v1/sessions/ses_unit_item_delivery/item-deliveries HTTP/1.1\r\nIdempotency-Key: 99\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
                &mut runtime
            )
            .status(),
            400
        );
        assert_eq!(
            handle_item_delivery_http_request(
                "POST /v1/sessions/ses_unit_item_delivery/item-deliveries HTTP/1.1\r\nIdempotency-Key: dlv_ok\r\nContent-Type: application/json\r\nContent-Length: 8\r\n\r\nshort",
                &mut runtime
            )
            .status(),
            400
        );
        let mismatch = "{\"delivery_ref\":\"dlv_a\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";
        assert_eq!(
            handle_item_delivery_http_request(&post(mismatch, "dlv_b"), &mut runtime).status(),
            400
        );
        let numeric_item = "{\"delivery_ref\":\"dlv_ok\",\"item_version_ref\":\"123\",\"presentation_context_ref\":\"presentation_web_v1\"}";
        assert_eq!(
            handle_item_delivery_http_request(&post(numeric_item, "dlv_ok"), &mut runtime).status(),
            400
        );
        let first = "{\"delivery_ref\":\"dlv_first\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";
        assert_eq!(
            handle_item_delivery_http_request(&post(first, "dlv_first"), &mut runtime).status(),
            201
        );
        let duplicate = "{\"delivery_ref\":\"dlv_second\",\"item_version_ref\":\"item_version_001\",\"presentation_context_ref\":\"presentation_web_v1\"}";
        assert_eq!(
            handle_item_delivery_http_request(&post(duplicate, "dlv_second"), &mut runtime)
                .status(),
            409
        );
    }
}
