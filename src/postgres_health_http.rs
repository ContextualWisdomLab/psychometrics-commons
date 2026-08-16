//! Compose live `PostgreSQL` operational snapshots into operator HTTP probes.
//!
//! GET `/live` answers process liveness without store I/O. GET `/ready`
//! observes the caller-owned connection after the request is accepted and
//! reuses the transport-neutral health HTTP translator. The adapter does not
//! invent backlog thresholds or expose raw driver errors.

use crate::health::{BacklogHealth, DataIntegrityHealth, RuntimeHealthSnapshot};
use crate::health_http::{
    accept_one_health_http_with, handle_health_http_request, health_ready_response,
    health_request_required_capabilities, health_request_requires_readiness_snapshot,
    serve_health_http_with, HealthHttpResponse,
};
use crate::postgres_health::{
    observe_postgres_operational_snapshot, POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF,
};
use postgres::GenericClient;
use std::io;
use std::net::TcpListener;

/// Observe `PostgreSQL` only when answering GET `/ready`.
///
/// GET `/live` and RFC 9457 problem responses use a process-liveness snapshot
/// and never touch the caller-owned connection. Bare GET `/ready` requires
/// `postgres_operational_store` so a read-only or unsupported store cannot
/// advertise readiness to a load balancer that omits `capability=`.
#[must_use]
pub fn handle_postgres_health_http_request(
    request: &str,
    client: &mut impl GenericClient,
    required_relations: &[&str],
    backlog_health: BacklogHealth,
) -> HealthHttpResponse {
    if !health_request_requires_readiness_snapshot(request) {
        return handle_health_http_request(request, &process_liveness_snapshot());
    }
    let snapshot =
        observe_postgres_operational_snapshot(client, required_relations, backlog_health);
    let required =
        postgres_ready_required_capabilities(health_request_required_capabilities(request));
    health_ready_response(&snapshot, &required)
}

fn postgres_ready_required_capabilities(named: Vec<&str>) -> Vec<&str> {
    if named.is_empty() {
        vec![POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]
    } else {
        named
    }
}

/// Accept one TCP connection, then observe `PostgreSQL` only if the request is GET `/ready`.
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
    accept_one_health_http_with(listener, |request| {
        handle_postgres_health_http_request(request, client, required_relations, backlog_health)
    })
}

/// Serve PostgreSQL-backed probes until `accept` fails.
///
/// GET `/live` still answers without store I/O. GET `/ready` observes the
/// caller-owned connection after each accept.
///
/// # Errors
///
/// Returns the I/O error that stopped the loop.
pub fn serve_postgres_health_http(
    listener: &TcpListener,
    client: &mut impl GenericClient,
    required_relations: &[&str],
    backlog_health: BacklogHealth,
) -> io::Result<()> {
    serve_health_http_with(listener, |request| {
        handle_postgres_health_http_request(request, client, required_relations, backlog_health)
    })
}

fn process_liveness_snapshot() -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::Unknown,
        DataIntegrityHealth::Unknown,
        Vec::new(),
    )
    .expect("empty process-liveness snapshot is valid")
}

#[cfg(test)]
mod tests {
    use super::{postgres_ready_required_capabilities, process_liveness_snapshot};
    use crate::health::{BacklogHealth, DataIntegrityHealth};
    use crate::postgres_health::POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF;

    #[test]
    fn process_liveness_snapshot_does_not_claim_readiness() {
        let snapshot = process_liveness_snapshot();
        assert!(snapshot.is_live());
        assert_eq!(snapshot.backlog_health(), BacklogHealth::Unknown);
        assert_eq!(
            snapshot.data_integrity_health(),
            DataIntegrityHealth::Unknown
        );
        assert!(snapshot.capabilities().is_empty());
        assert!(!snapshot.is_ready_for(&[]));
    }

    #[test]
    fn bare_ready_requires_the_postgres_operational_store() {
        assert_eq!(
            postgres_ready_required_capabilities(Vec::new()),
            vec![POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]
        );
        assert_eq!(
            postgres_ready_required_capabilities(vec!["scoring"]),
            vec!["scoring"]
        );
    }
}
