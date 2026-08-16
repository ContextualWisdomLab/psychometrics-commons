//! Operator HTTP probes for process liveness and operation-scoped readiness.
//!
//! These probes translate [`RuntimeHealthSnapshot`] into load-balancer-safe
//! HTTP responses. They do not invent availability SLOs, execute store I/O, or
//! expose raw database or provider errors.

use crate::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, RuntimeHealthSnapshot,
};
use std::fmt::Write;

/// Process-liveness probe path.
pub const HEALTH_LIVE_PATH: &str = "/live";
/// Operation-readiness probe path.
pub const HEALTH_READY_PATH: &str = "/ready";

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
        HEALTH_READY_PATH => {
            let required = required_capabilities(query);
            let ready = snapshot.is_ready_for(&required);
            let status = if ready { 200 } else { 503 };
            HealthHttpResponse::json(status, snapshot_body(snapshot, ready))
        }
        _ => HealthHttpResponse::problem(
            404,
            "Not Found",
            "health probes accept GET /live and GET /ready only",
        ),
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
        backlog_label, capability_state_label, handle_health_http_request, integrity_label,
        json_string, HEALTH_LIVE_PATH,
    };
    use crate::health::{
        BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth,
        RuntimeHealthSnapshot,
    };

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
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("a\n\r\t"), "\"a\\n\\r\\t\"");
        assert_eq!(json_string("\u{0001}"), "\"\\u0001\"");
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
    }
}
