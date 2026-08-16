//! Compose live `PostgreSQL` operational snapshots into operator HTTP probes.
//!
//! This adapter observes the caller-owned connection and then reuses the
//! transport-neutral health HTTP translator. It does not invent backlog
//! thresholds or expose raw driver errors on `/live` or `/ready`.

use crate::health::BacklogHealth;
use crate::health_http::{accept_one_health_http, handle_health_http_request, HealthHttpResponse};
use crate::postgres_health::observe_postgres_operational_snapshot;
use postgres::GenericClient;
use std::io;
use std::net::TcpListener;

/// Observe `PostgreSQL` and answer one operator health HTTP request.
#[must_use]
pub fn handle_postgres_health_http_request(
    request: &str,
    client: &mut impl GenericClient,
    required_relations: &[&str],
    backlog_health: BacklogHealth,
) -> HealthHttpResponse {
    let snapshot =
        observe_postgres_operational_snapshot(client, required_relations, backlog_health);
    handle_health_http_request(request, &snapshot)
}

/// Accept one TCP connection after observing current `PostgreSQL` health.
///
/// # Errors
///
/// Returns the I/O error if accept, read, or write fails.
pub fn accept_one_postgres_health_http(
    listener: &TcpListener,
    client: &mut impl GenericClient,
    required_relations: &[&str],
    backlog_health: BacklogHealth,
) -> io::Result<()> {
    let snapshot =
        observe_postgres_operational_snapshot(client, required_relations, backlog_health);
    accept_one_health_http(listener, &snapshot)
}
