//! Public HTTP application behavior for item-delivery evidence.
//!
//! This module exposes `POST` and `GET`
//! `/v1/sessions/{session_ref}/item-deliveries`. It records only which immutable
//! published item version was presented and reloads the accepted server order.
//! Item selection, calibration, scoring, and other psychometric numerics remain
//! outside this transport. The runtime keeps the authoritative
//! [`AssessmentSession`] aggregate beside its ledger; callers cannot replace the
//! session lifecycle with a detached state value.

use crate::item_delivery::{ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest};
use crate::reference::normalized_reference;
use crate::session::AssessmentSession;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter, Write};
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

/// Path suffix under one assessment session for item-delivery evidence.
pub const ITEM_DELIVERY_COLLECTION_SUFFIX: &str = "/item-deliveries";
/// Overall read/write timeout used by the hardened socket boundary.
pub const ITEM_DELIVERY_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum accepted request size, including request line, headers, and body.
pub const ITEM_DELIVERY_HTTP_MAX_REQUEST_BYTES: usize = 8_192;

struct DeliverySession {
    session: AssessmentSession,
    ledger: ItemDeliveryLedger,
}

/// Error returned when injected runtime evidence is not one coherent session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemDeliveryHttpRuntimeError {
    /// The session and ledger name different session or release evidence.
    SessionLedgerMismatch,
}

impl Display for ItemDeliveryHttpRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("item-delivery runtime session and ledger evidence must match exactly")
    }
}

impl Error for ItemDeliveryHttpRuntimeError {}

/// In-process authoritative sessions and their item-delivery ledgers.
#[derive(Default)]
pub struct ItemDeliveryHttpRuntime {
    sessions: HashMap<String, DeliverySession>,
}

impl ItemDeliveryHttpRuntime {
    /// Create an empty item-delivery HTTP runtime.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one authoritative session and its exact bound delivery ledger.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryHttpRuntimeError::SessionLedgerMismatch`] if session
    /// identity, release identity, release digest, or locale differs. Existing
    /// session identity is also rejected so seeding cannot silently replace live
    /// evidence.
    pub fn insert_session(
        &mut self,
        session: AssessmentSession,
        ledger: ItemDeliveryLedger,
    ) -> Result<(), ItemDeliveryHttpRuntimeError> {
        if session.session_ref() != ledger.session_ref()
            || session.instrument_release_ref() != ledger.instrument_release_ref()
            || session.instrument_release_content_digest() != ledger.release_content_digest()
            || session.locale() != ledger.locale()
            || self.sessions.contains_key(session.session_ref())
        {
            return Err(ItemDeliveryHttpRuntimeError::SessionLedgerMismatch);
        }
        self.sessions.insert(
            session.session_ref().to_owned(),
            DeliverySession { session, ledger },
        );
        Ok(())
    }

    /// Return the number of accepted delivery events for one session.
    #[must_use]
    pub fn event_count(&self, session_ref: &str) -> usize {
        self.sessions
            .get(session_ref)
            .map_or(0, |entry| entry.ledger.len())
    }
}

/// HTTP response produced by the item-delivery public operation family.
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

/// Translate one complete HTTP/1.1 request into record or reload behavior.
///
/// The hardened socket framing owner lives in `item_delivery_http_boundary.rs`.
/// Direct calls still reject duplicate semantic headers, transfer encoding,
/// malformed content length, encoded path aliases, and non-canonical identities.
#[must_use]
pub fn handle_item_delivery_http_request(
    request: &str,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    let Some((method, target)) = parse_request_line(request) else {
        return bad_request("Send one HTTP/1.1 request with an exact method and target");
    };
    if target.contains('?') || target.contains('%') {
        return not_found();
    }
    let Some(session_ref) = collection_session_ref(target) else {
        return not_found();
    };
    match method {
        "POST" => handle_record(request, session_ref, runtime),
        "GET" => handle_list(session_ref, runtime),
        _ => method_not_allowed(),
    }
}

/// Bind a blocking TCP listener for the item-delivery operation family.
///
/// The returned listener does not accept until the hardened boundary's
/// `accept_one_item_delivery_http` is called.
///
/// # Errors
///
/// Returns the operating-system bind error.
pub fn bind_item_delivery_http(addr: SocketAddr) -> io::Result<TcpListener> {
    TcpListener::bind(addr)
}

fn collection_session_ref(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v1/sessions/")?;
    let (session_ref, suffix) = rest.split_once('/')?;
    if suffix != "item-deliveries" || session_ref.is_empty() || session_ref.contains('/') {
        return None;
    }
    normalized_reference(session_ref).filter(|normalized| *normalized == session_ref)
}

fn handle_record(
    request: &str,
    session_ref: &str,
    runtime: &mut ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    if header_value(request, "transfer-encoding").is_some() {
        return bad_request("Resend the request without Transfer-Encoding");
    }
    let Some(idempotency_key) = unique_header_value(request, "idempotency-key")
        .and_then(valid_reference)
    else {
        return ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:missing-idempotency-key",
            "Missing Idempotency Key",
            "Send one exact opaque Idempotency-Key header matching delivery_ref",
        );
    };
    if unique_header_value(request, "content-type") != Some("application/json") {
        return ItemDeliveryHttpResponse::problem(
            415,
            "urn:psychometrics-commons:problem:unsupported-media-type",
            "Unsupported Media Type",
            "Resend with Content-Type: application/json",
        );
    }
    let Some(body) = request_body(request) else {
        return bad_request("Send one complete JSON object with an exact byte Content-Length");
    };
    let Some(record) = parse_record_body(body) else {
        return bad_request(
            "Send delivery_ref, item_version_ref, presentation_context_ref, and optional selection_evidence_ref as JSON strings only",
        );
    };
    if record.delivery_ref != idempotency_key {
        return bad_request("Make Idempotency-Key exactly match delivery_ref");
    }
    let Some(entry) = runtime.sessions.get_mut(session_ref) else {
        return session_not_found();
    };
    let before = entry.ledger.len();
    let result = entry.ledger.deliver(
        entry.session.state(),
        ItemDeliveryRequest {
            delivery_ref: &record.delivery_ref,
            item_version_ref: &record.item_version_ref,
            presentation_context_ref: &record.presentation_context_ref,
            selection_evidence_ref: record.selection_evidence_ref.as_deref(),
        },
    );
    match result {
        Ok(event) => ItemDeliveryHttpResponse::json(
            if entry.ledger.len() == before { 200 } else { 201 },
            event_body(&entry.ledger, &event),
        ),
        Err(error) => delivery_problem(error),
    }
}

fn handle_list(
    session_ref: &str,
    runtime: &ItemDeliveryHttpRuntime,
) -> ItemDeliveryHttpResponse {
    match runtime.sessions.get(session_ref) {
        Some(entry) => ItemDeliveryHttpResponse::json(200, ledger_body(entry)),
        None => session_not_found(),
    }
}

fn bad_request(detail: &str) -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        400,
        "urn:psychometrics-commons:problem:bad-request",
        "Bad Request",
        detail,
    )
}

fn not_found() -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        404,
        "urn:psychometrics-commons:problem:not-found",
        "Not Found",
        "Use POST or GET /v1/sessions/{session_ref}/item-deliveries with the exact session reference",
    )
}

fn session_not_found() -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        404,
        "urn:psychometrics-commons:problem:session-not-found",
        "Session Not Found",
        "Confirm the exact assessment session exists before recording or reloading item deliveries",
    )
}

fn method_not_allowed() -> ItemDeliveryHttpResponse {
    ItemDeliveryHttpResponse::problem(
        405,
        "urn:psychometrics-commons:problem:method-not-allowed",
        "Method Not Allowed",
        "Use POST to record or GET to reload item-delivery evidence",
    )
}

fn delivery_problem(error: ItemDeliveryError) -> ItemDeliveryHttpResponse {
    match error {
        ItemDeliveryError::InvalidReference => ItemDeliveryHttpResponse::problem(
            400,
            "urn:psychometrics-commons:problem:invalid-reference",
            "Invalid Reference",
            "Use exact opaque non-numeric item-delivery references",
        ),
        ItemDeliveryError::SessionNotActive(_) => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:session-not-active",
            "Session Not Active",
            "Activate the session before presenting a new item",
        ),
        ItemDeliveryError::IdempotencyConflict => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:idempotency-conflict",
            "Idempotency Conflict",
            "Replay this delivery_ref with its exact original evidence or use a new delivery_ref",
        ),
        ItemDeliveryError::ItemNotInRelease => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:item-not-in-release",
            "Item Not In Release",
            "Use an item_version_ref from the session's exact published release",
        ),
        ItemDeliveryError::DuplicateItemDelivery => ItemDeliveryHttpResponse::problem(
            409,
            "urn:psychometrics-commons:problem:duplicate-item-delivery",
            "Duplicate Item Delivery",
            "Reload the delivery ledger instead of presenting the same item under a new delivery_ref",
        ),
    }
}

struct RecordBody {
    delivery_ref: String,
    item_version_ref: String,
    presentation_context_ref: String,
    selection_evidence_ref: Option<String>,
}

fn parse_record_body(body: &str) -> Option<RecordBody> {
    let fields = parse_string_object(body)?;
    if !(3..=4).contains(&fields.len()) {
        return None;
    }
    let selection_evidence_ref = fields.get("selection_evidence_ref").cloned();
    if fields.len() == 4 && selection_evidence_ref.is_none() {
        return None;
    }
    Some(RecordBody {
        delivery_ref: fields.get("delivery_ref")?.clone(),
        item_version_ref: fields.get("item_version_ref")?.clone(),
        presentation_context_ref: fields.get("presentation_context_ref")?.clone(),
        selection_evidence_ref,
    })
}

fn parse_string_object(body: &str) -> Option<HashMap<String, String>> {
    let inner = body.trim().strip_prefix('{')?.strip_suffix('}')?;
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

fn unique_header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    let mut found = None;
    for line in request.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        let (header_name, value) = line.split_once(':')?;
        if header_name.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return None;
            }
            found = Some(value.trim_matches(&[' ', '\t'][..]));
        }
    }
    found
}

fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    unique_header_value(request, name)
}

fn valid_reference(value: &str) -> Option<&str> {
    normalized_reference(value).filter(|normalized| *normalized == value)
}

fn request_body(request: &str) -> Option<&str> {
    let (headers, body) = request.split_once("\r\n\r\n")?;
    let length = unique_header_value(headers, "content-length")?
        .parse::<usize>()
        .ok()?;
    if body.len() != length {
        return None;
    }
    body.get(..length)
}

fn parse_request_line(request: &str) -> Option<(&str, &str)> {
    let line = request.split("\r\n").next()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    if parts.next()? != "HTTP/1.1" || parts.next().is_some() || method.is_empty() || target.is_empty() {
        return None;
    }
    Some((method, target))
}

fn event_body(ledger: &ItemDeliveryLedger, event: &crate::item_delivery::ItemDeliveryEvent) -> String {
    let selection = event
        .selection_evidence_ref()
        .map(|value| format!(",\"selection_evidence_ref\":{}", json_string(value)))
        .unwrap_or_default();
    format!(
        "{{\"session_ref\":{},\"instrument_release_ref\":{},\"locale\":{},\"delivery_ref\":{},\"item_version_ref\":{},\"presentation_context_ref\":{}{selection},\"sequence\":{}}}",
        json_string(ledger.session_ref()),
        json_string(ledger.instrument_release_ref()),
        json_string(ledger.locale()),
        json_string(event.delivery_ref()),
        json_string(event.item_version_ref()),
        json_string(event.presentation_context_ref()),
        event.sequence()
    )
}

fn ledger_body(entry: &DeliverySession) -> String {
    let events = entry
        .ledger
        .events()
        .iter()
        .map(|event| event_body(&entry.ledger, event))
        .collect::<Vec<_>>()
        .join(",");
    let allowed = entry
        .ledger
        .allowed_item_version_refs()
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"session_ref\":{},\"instrument_release_ref\":{},\"release_content_digest\":{},\"locale\":{},\"session_state\":{},\"allowed_item_version_refs\":[{allowed}],\"events\":[{events}]}}",
        json_string(entry.ledger.session_ref()),
        json_string(entry.ledger.instrument_release_ref()),
        json_string(entry.ledger.release_content_digest()),
        json_string(entry.ledger.locale()),
        json_string(entry.session.state().persist_name())
    )
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
    use super::{json_string, parse_json_string, parse_record_body, parse_request_line, parse_string_object};

    #[test]
    fn parser_helpers_reject_ambiguous_objects_and_escape_json() {
        assert!(parse_string_object("{}").unwrap().is_empty());
        assert!(parse_string_object("{\"a\":\"1\",\"a\":\"2\"}").is_none());
        assert!(parse_string_object("{\"a\":\"1\",}").is_none());
        assert!(parse_record_body("{\"delivery_ref\":\"d\"}").is_none());
        assert!(parse_record_body("{\"delivery_ref\":\"d\",\"item_version_ref\":\"i\",\"presentation_context_ref\":\"p\",\"other\":\"x\"}").is_none());
        assert!(parse_json_string("\"unterminated").is_none());
        assert!(parse_json_string("\"bad\\x\"").is_none());
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
        assert!(parse_request_line("GET / HTTP/1.0\r\n\r\n").is_none());
        assert_eq!(parse_request_line("GET / HTTP/1.1\r\n\r\n"), Some(("GET", "/")));
    }
}
