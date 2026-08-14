//! Physical integrity contract for durable outbox lease fencing evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema = format!("outbox_lease_fence_integrity_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

#[test]
fn current_lease_fencing_token_must_equal_persisted_generation() {
    let mut client = ready_client();
    client
        .execute(
            "INSERT INTO integration_outbox (
                 event_ref, event_type, schema_version, source_ref, tenant_ref,
                 subject_ref, occurred_at_unix_ms, correlation_ref, payload_digest,
                 max_attempts, latest_event_at_unix_ms
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$7)",
            &[
                &"event_alpha",
                &"assessment.completed",
                &"v1",
                &"psychometrics_commons",
                &"tenant_alpha",
                &"subject_alpha",
                &10_000_i64,
                &"correlation_alpha",
                &"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &3_i32,
            ],
        )
        .unwrap();

    let error = client
        .execute(
            "UPDATE integration_outbox
             SET lease_worker_ref = 'worker_alpha',
                 lease_ref = 'lease_alpha',
                 lease_fencing_token = 2,
                 lease_expires_at_unix_ms = 20_000,
                 delivery_lease_generation = 1
             WHERE source_ref = 'psychometrics_commons'
               AND tenant_ref = 'tenant_alpha'
               AND event_ref = 'event_alpha'",
            &[],
        )
        .expect_err("current lease token must be the current delivery lease generation");
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::CHECK_VIOLATION)
    );
}
