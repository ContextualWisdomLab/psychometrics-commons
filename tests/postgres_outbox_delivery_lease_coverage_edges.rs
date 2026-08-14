//! Coverage-visible fail-closed edges for fenced outbox delivery authority.
//!
//! These tests exercise two operational states that are deliberately outside the
//! normal happy path: an exact immutable attempt row that already exists while its
//! matching lease remains present, and a database-side clock lookup that fails after
//! the outbox row has been locked. Both cases must remain deterministic and fail-safe.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent, OutboxState};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    record_leased_outbox_delivery_attempt, OutboxPersistenceIdentity, PersistenceDisposition,
    PersistenceError,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "outbox_delivery_lease_coverage_edge_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_4345;

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL coverage-edge lock should be acquired");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

fn cleanup(client: &mut Client) {
    client
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"))
        .unwrap();
}

fn database_now_unix_ms(client: &mut Client) -> u64 {
    let value: i64 = client
        .query_one(
            "SELECT floor(extract(epoch FROM pg_catalog.clock_timestamp()) * 1000)::bigint",
            &[],
        )
        .unwrap()
        .get(0);
    u64::try_from(value).unwrap()
}

fn identity(event_ref: &str) -> OutboxPersistenceIdentity<'_> {
    OutboxPersistenceIdentity::new("psychometrics_commons", "tenant_alpha", event_ref)
}

fn enqueue_and_claim(client: &mut Client, event_ref: &str) -> (u64, u64) {
    let now = database_now_unix_ms(client);
    let event_time = now - 10_000;
    let event = IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        event_time,
        "correlation_alpha",
        None,
        DIGEST,
    )
    .unwrap();
    assert_eq!(
        enqueue_outbox_event(client, &event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );

    let mut transaction = client.transaction().unwrap();
    let lease = claim_outbox_delivery(
        &mut transaction,
        identity(event_ref),
        "worker_coverage_edge",
        "outbox_lease_coverage_edge",
        now,
        now + 60_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    (lease.fencing_token(), event_time)
}

#[test]
fn exact_attempt_replay_with_live_matching_fence_is_idempotent() {
    let mut client = ready_client();
    let (fence, event_time) = enqueue_and_claim(&mut client, "event_live_exact_replay");
    let attempt_time = event_time + 1;

    client
        .execute(
            "INSERT INTO integration_delivery_attempt (
                 source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome,
                 occurred_at_unix_ms, cause_code
             ) VALUES (
                 'psychometrics_commons', 'tenant_alpha', 'event_live_exact_replay',
                 'attempt_live_exact_replay', 'delivered', $1, NULL
             )",
            &[&i64::try_from(attempt_time).unwrap()],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let replay = record_leased_outbox_delivery_attempt(
        &mut transaction,
        identity("event_live_exact_replay"),
        "attempt_live_exact_replay",
        DeliveryOutcome::Delivered,
        attempt_time,
        None,
        fence,
    )
    .unwrap();
    assert_eq!(replay.disposition(), PersistenceDisposition::Duplicate);
    assert_eq!(replay.outbox_state(), OutboxState::Pending);
    transaction.rollback().unwrap();
    cleanup(&mut client);
}

#[test]
fn database_clock_lookup_failure_is_exposed_fail_closed() {
    let mut client = ready_client();
    let (fence, event_time) = enqueue_and_claim(&mut client, "event_clock_failure");

    client
        .batch_execute(&format!(
            "CREATE FUNCTION {SCHEMA}.clock_timestamp()
             RETURNS timestamp with time zone
             LANGUAGE plpgsql
             AS $$
             BEGIN
                 RAISE EXCEPTION 'forced clock failure for persistence error evidence';
             END;
             $$;
             SET search_path TO {SCHEMA}, pg_catalog;"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_clock_failure"),
            "attempt_clock_failure",
            DeliveryOutcome::Delivered,
            event_time + 1,
            None,
            fence,
        ),
        Err(PersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
    cleanup(&mut client);
}
