//! PostgreSQL contract for the verified publisher-to-fenced-persistence handoff.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent, OutboxState};
use psychometrics_commons_runtime::integration_delivery::{
    execute_verified_integration_publish, record_verified_leased_delivery_attempt,
};
use psychometrics_commons_runtime::integration_publisher::{
    IntegrationPublishReceipt, IntegrationPublisher,
};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    OutboxPersistenceIdentity, PersistenceDisposition, PersistenceError,
};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

const DATABASE_TEST_LOCK_KEY: i64 = 0x5652_4644_484E_4446;
const SOURCE: &str = "src_verified_handoff";
const TENANT: &str = "tnt_verified_handoff";
const SUBJECT: &str = "rsrc_verified_handoff";
const CORRELATION: &str = "cor_verified_handoff";

fn schema_name() -> String {
    format!("verified_handoff_{}", std::process::id())
}

fn now_unix_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_millis(),
    )
    .expect("test clock must fit the product timestamp range")
}

fn event(event_ref: &str, occurred_at_unix_ms: u64) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "result.released",
        "v1",
        SOURCE,
        TENANT,
        SUBJECT,
        occurred_at_unix_ms,
        CORRELATION,
        None,
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .unwrap()
}

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("verified handoff PostgreSQL test lock should be acquired");
    let schema = schema_name();
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;
             CREATE SCHEMA {schema};
             SET search_path TO {schema};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    client
}

fn cleanup(client: &mut Client) {
    let schema = schema_name();
    client
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE;"))
        .unwrap();
}

#[derive(Debug)]
struct PublisherUnavailable;

impl Display for PublisherUnavailable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("verified publisher unavailable")
    }
}

impl Error for PublisherUnavailable {}

struct DeliveredPublisher;

impl IntegrationPublisher for DeliveredPublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            integration_event,
            DeliveryOutcome::Delivered,
        ))
    }
}

#[test]
fn verified_handoff_records_only_its_own_fenced_outbox_identity() {
    let mut client = ready_client();
    let now = now_unix_ms();
    let primary = event("evt_verified_handoff_primary", now - 1_000);
    let other = event("evt_verified_handoff_other", now - 900);

    assert_eq!(
        enqueue_outbox_event(&mut client, &primary, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    assert_eq!(
        enqueue_outbox_event(&mut client, &other, 3).unwrap(),
        PersistenceDisposition::Inserted
    );

    let primary_identity =
        OutboxPersistenceIdentity::new(SOURCE, TENANT, primary.event_ref());
    let lease = {
        let mut transaction = client.transaction().unwrap();
        let lease = claim_outbox_delivery(
            &mut transaction,
            primary_identity,
            "worker_verified_handoff",
            "lease_verified_handoff",
            now,
            now + 60_000,
        )
        .unwrap();
        transaction.commit().unwrap();
        lease
    };

    let verified_primary = execute_verified_integration_publish(&DeliveredPublisher, &primary)
        .expect("the exact primary publisher acknowledgement should verify");
    let persistence = {
        let mut transaction = client.transaction().unwrap();
        let persistence = record_verified_leased_delivery_attempt(
            &mut transaction,
            &verified_primary,
            "attempt_verified_handoff",
            now + 1,
            None,
            lease.fencing_token(),
        )
        .unwrap();
        transaction.commit().unwrap();
        persistence
    };
    assert_eq!(persistence.disposition(), PersistenceDisposition::Inserted);
    assert_eq!(persistence.outbox_state(), OutboxState::Delivered);

    let primary_state: String = client
        .query_one(
            "SELECT current_state FROM integration_outbox
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[&SOURCE, &TENANT, &primary.event_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(primary_state, "delivered");

    let verified_other = execute_verified_integration_publish(&DeliveredPublisher, &other)
        .expect("the exact other publisher acknowledgement should verify");
    let mut transaction = client.transaction().unwrap();
    let error = record_verified_leased_delivery_attempt(
        &mut transaction,
        &verified_other,
        "attempt_wrong_fence_target",
        now + 2,
        None,
        lease.fencing_token(),
    )
    .expect_err("a fence for one outbox must not authorize another verified receipt");
    assert!(matches!(error, PersistenceError::NotLeased));
    transaction.rollback().unwrap();

    let other_state: String = client
        .query_one(
            "SELECT current_state FROM integration_outbox
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[&SOURCE, &TENANT, &other.event_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(other_state, "pending");

    cleanup(&mut client);
}
