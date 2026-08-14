//! Database-authoritative lease expiry and fencing precedence for outbox delivery.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    record_leased_outbox_delivery_attempt, OutboxPersistenceIdentity, PersistenceDisposition,
    PersistenceError,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "outbox_delivery_lease_authority_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_4155;

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL outbox authority test lock should be acquired");
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
            "SELECT floor(extract(epoch FROM clock_timestamp()) * 1000)::bigint",
            &[],
        )
        .unwrap()
        .get(0);
    u64::try_from(value).unwrap()
}

fn identity(event_ref: &str) -> OutboxPersistenceIdentity<'_> {
    OutboxPersistenceIdentity::new("psychometrics_commons", "tenant_alpha", event_ref)
}

fn event(event_ref: &str, occurred_at_unix_ms: u64) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        occurred_at_unix_ms,
        "correlation_alpha",
        None,
        DIGEST,
    )
    .unwrap()
}

fn enqueue(client: &mut Client, event_ref: &str, occurred_at_unix_ms: u64) {
    assert_eq!(
        enqueue_outbox_event(client, &event(event_ref, occurred_at_unix_ms), 3).unwrap(),
        PersistenceDisposition::Inserted
    );
}

fn claim(
    client: &mut Client,
    event_ref: &str,
    worker_ref: &str,
    lease_ref: &str,
    claimed_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> u64 {
    let mut transaction = client.transaction().unwrap();
    let lease = claim_outbox_delivery(
        &mut transaction,
        identity(event_ref),
        worker_ref,
        lease_ref,
        claimed_at_unix_ms,
        expires_at_unix_ms,
    )
    .unwrap();
    transaction.commit().unwrap();
    lease.fencing_token()
}

#[test]
fn expired_lease_is_classified_by_database_time_not_stale_worker_time() {
    let mut client = ready_client();
    let now = database_now_unix_ms(&mut client);
    let event_time = now - 10_000;
    enqueue(&mut client, "event_expired_by_server_clock", event_time);
    let fence = claim(
        &mut client,
        "event_expired_by_server_clock",
        "worker_expired",
        "outbox_lease_expired",
        now - 5_000,
        now - 4_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_expired_by_server_clock"),
            "attempt_stale_worker_clock",
            DeliveryOutcome::Delivered,
            event_time + 1,
            None,
            fence,
        ),
        Err(PersistenceError::LeaseExpired)
    ));
    transaction.rollback().unwrap();
    cleanup(&mut client);
}

#[test]
fn stale_fence_is_rejected_before_exact_attempt_replay_is_classified() {
    let mut client = ready_client();
    let now = database_now_unix_ms(&mut client);
    let event_time = now - 10_000;
    enqueue(&mut client, "event_stale_replay", event_time);
    let first_fence = claim(
        &mut client,
        "event_stale_replay",
        "worker_first",
        "outbox_lease_first",
        now,
        now + 60_000,
    );

    {
        let mut transaction = client.transaction().unwrap();
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_stale_replay"),
            "attempt_retry",
            DeliveryOutcome::RetryableFailure,
            event_time + 1,
            Some("provider_unavailable"),
            first_fence,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    let second_fence = claim(
        &mut client,
        "event_stale_replay",
        "worker_second",
        "outbox_lease_second",
        now + 1,
        now + 60_001,
    );
    assert!(second_fence > first_fence);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_stale_replay"),
            "attempt_retry",
            DeliveryOutcome::RetryableFailure,
            event_time + 1,
            Some("provider_unavailable"),
            first_fence,
        ),
        Err(PersistenceError::StaleLease)
    ));
    transaction.rollback().unwrap();
    cleanup(&mut client);
}
