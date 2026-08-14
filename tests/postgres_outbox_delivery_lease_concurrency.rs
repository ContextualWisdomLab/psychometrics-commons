//! Concurrency acceptance for exclusive outbox delivery claims and fencing tokens.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    expire_outbox_delivery_lease, OutboxPersistenceIdentity, PersistenceDisposition,
    PersistenceError,
};
use std::sync::{Arc, Barrier};
use std::thread;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "outbox_delivery_lease_concurrency_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_434E;

#[derive(Debug, Eq, PartialEq)]
enum ClaimOutcome {
    Claimed(u64),
    NotLeaseable,
}

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn database_test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL concurrency test advisory lock should be acquired");
    client
}

fn schema_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(&format!("SET search_path TO {SCHEMA};"))
        .expect("worker connection should select the isolated test schema");
    client
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_concurrent_claim",
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        None,
        DIGEST,
    )
    .expect("concurrency fixture should satisfy the integration-event contract")
}

fn identity() -> OutboxPersistenceIdentity<'static> {
    OutboxPersistenceIdentity::new(
        "psychometrics_commons",
        "tenant_alpha",
        "event_concurrent_claim",
    )
}

fn race_claim(
    barrier: &Barrier,
    worker_ref: &'static str,
    lease_ref: &'static str,
) -> ClaimOutcome {
    let mut client = schema_client();
    let mut transaction = client
        .transaction()
        .expect("worker transaction should begin");
    barrier.wait();

    match claim_outbox_delivery(
        &mut transaction,
        identity(),
        worker_ref,
        lease_ref,
        10_000,
        11_000,
    ) {
        Ok(lease) => {
            let fencing_token = lease.fencing_token();
            transaction
                .commit()
                .expect("winning delivery claim should commit");
            ClaimOutcome::Claimed(fencing_token)
        }
        Err(PersistenceError::NotLeaseable) => {
            transaction
                .rollback()
                .expect("losing delivery claim should roll back cleanly");
            ClaimOutcome::NotLeaseable
        }
        Err(error) => panic!("concurrent delivery claim failed unexpectedly: {error:?}"),
    }
}

#[test]
fn concurrent_claims_have_one_winner_and_reclaim_advances_the_fence() {
    let _database_guard = database_test_guard();
    let mut setup_client = connect_client();
    setup_client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated concurrency schema should be reset");
    apply_integration_migration(&mut setup_client)
        .expect("integration migration should install the outbox schema");
    assert_eq!(
        enqueue_outbox_event(&mut setup_client, &event(), 3)
            .expect("pending event should be enqueued"),
        PersistenceDisposition::Inserted
    );

    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = Arc::clone(&barrier);
    let second_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        race_claim(
            &first_barrier,
            "worker_concurrent_alpha",
            "outbox_lease_concurrent_alpha",
        )
    });
    let second = thread::spawn(move || {
        race_claim(
            &second_barrier,
            "worker_concurrent_beta",
            "outbox_lease_concurrent_beta",
        )
    });

    let outcomes = [
        first.join().expect("first claim worker should not panic"),
        second.join().expect("second claim worker should not panic"),
    ];
    let claimed_tokens: Vec<u64> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            ClaimOutcome::Claimed(token) => Some(*token),
            ClaimOutcome::NotLeaseable => None,
        })
        .collect();
    let rejected_claims = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, ClaimOutcome::NotLeaseable))
        .count();
    assert_eq!(claimed_tokens, vec![1]);
    assert_eq!(rejected_claims, 1);

    let mut expiry_transaction = setup_client
        .transaction()
        .expect("expiry transaction should begin");
    expire_outbox_delivery_lease(&mut expiry_transaction, identity(), 11_000)
        .expect("the committed winning lease should expire at its boundary");
    expiry_transaction
        .commit()
        .expect("expiry recovery should commit");

    let mut reclaim_transaction = setup_client
        .transaction()
        .expect("reclaim transaction should begin");
    let reclaimed = claim_outbox_delivery(
        &mut reclaim_transaction,
        identity(),
        "worker_concurrent_recovered",
        "outbox_lease_concurrent_recovered",
        11_000,
        12_000,
    )
    .expect("recovered pending event should be claimable");
    assert_eq!(reclaimed.fencing_token(), 2);
    reclaim_transaction
        .commit()
        .expect("recovered claim should commit");

    setup_client
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"))
        .expect("isolated concurrency schema should be removed");
}
