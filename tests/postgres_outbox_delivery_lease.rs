//! Exclusive outbox delivery leases recover expired workers without transferring the fence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent, OutboxState};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, claim_outbox_delivery, enqueue_outbox_event,
    expire_outbox_delivery_lease, record_leased_outbox_delivery_attempt,
    record_outbox_delivery_attempt, OutboxPersistenceIdentity, PersistenceDisposition,
    PersistenceError,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCHEMA: &str = "outbox_delivery_lease_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_4C53;

fn database_test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL outbox-lease test advisory lock should be acquired");
    client
}

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn test_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    client
}

fn reset_integration_tables(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {SCHEMA}.integration_inbox CASCADE;
             DROP TABLE IF EXISTS {SCHEMA}.integration_delivery_attempt CASCADE;
             DROP TABLE IF EXISTS {SCHEMA}.integration_outbox CASCADE;"
        ))
        .unwrap();
}

fn event(event_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
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
    .unwrap()
}

fn identity(event_ref: &str) -> OutboxPersistenceIdentity<'_> {
    OutboxPersistenceIdentity::new("psychometrics_commons", "tenant_alpha", event_ref)
}

fn enqueue(client: &mut Client, event_ref: &str) {
    assert_eq!(
        enqueue_outbox_event(client, &event(event_ref), 3).unwrap(),
        PersistenceDisposition::Inserted
    );
}

fn enqueue_and_claim(
    client: &mut Client,
    event_ref: &str,
    worker_ref: &str,
    lease_ref: &str,
    expires_at_unix_ms: u64,
) -> u64 {
    enqueue(client, event_ref);
    let mut transaction = client.transaction().unwrap();
    let lease = claim_outbox_delivery(
        &mut transaction,
        identity(event_ref),
        worker_ref,
        lease_ref,
        10_000,
        expires_at_unix_ms,
    )
    .unwrap();
    transaction.commit().unwrap();
    lease.fencing_token()
}

fn lease_row(client: &mut Client, event_ref: &str) -> (Option<String>, Option<i64>, String) {
    let row = client
        .query_one(
            "SELECT lease_ref, lease_fencing_token, current_state
             FROM integration_outbox
             WHERE source_ref = 'psychometrics_commons'
               AND tenant_ref = 'tenant_alpha'
               AND event_ref = $1",
            &[&event_ref],
        )
        .unwrap();
    (row.get(0), row.get(1), row.get(2))
}

#[test]
fn expired_delivery_lease_recovers_and_reclaim_issues_next_fence() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    let first_fence = enqueue_and_claim(
        &mut client,
        "event_expired_retry",
        "worker_expired",
        "outbox_lease_expired",
        11_000,
    );
    assert_eq!(first_fence, 1);

    {
        let mut transaction = client.transaction().unwrap();
        expire_outbox_delivery_lease(&mut transaction, identity("event_expired_retry"), 11_000)
            .unwrap();
        transaction.commit().unwrap();
    }

    assert_eq!(
        lease_row(&mut client, "event_expired_retry"),
        (None, None, "pending".to_owned())
    );

    let mut transaction = client.transaction().unwrap();
    let recovered = claim_outbox_delivery(
        &mut transaction,
        identity("event_expired_retry"),
        "worker_recovered",
        "outbox_lease_recovered",
        11_000,
        12_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(recovered.fencing_token(), 2);
    assert_eq!(recovered.worker_ref(), "worker_recovered");
    assert_eq!(recovered.lease_ref(), "outbox_lease_recovered");
    assert_eq!(recovered.expires_at_unix_ms(), 12_000);
}

#[test]
fn fenced_retryable_failure_clears_lease_for_the_next_claim() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    let fence = enqueue_and_claim(
        &mut client,
        "event_fenced_retry",
        "worker_retry",
        "outbox_lease_retry",
        20_000,
    );

    let mut transaction = client.transaction().unwrap();
    let retried = record_leased_outbox_delivery_attempt(
        &mut transaction,
        identity("event_fenced_retry"),
        "attempt_retry",
        DeliveryOutcome::RetryableFailure,
        10_001,
        Some("provider_unavailable"),
        fence,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(retried.outbox_state(), OutboxState::Pending);
    assert_eq!(
        lease_row(&mut client, "event_fenced_retry"),
        (None, None, "pending".to_owned())
    );

    let mut transaction = client.transaction().unwrap();
    let next = claim_outbox_delivery(
        &mut transaction,
        identity("event_fenced_retry"),
        "worker_next",
        "outbox_lease_next",
        10_002,
        21_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(next.fencing_token(), 2);
}

#[test]
fn claim_does_not_steal_an_unrecovered_expired_lease() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue_and_claim(
        &mut client,
        "event_unrecovered",
        "worker_expired",
        "outbox_lease_expired",
        11_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_unrecovered"),
            "worker_thief",
            "outbox_lease_thief",
            12_000,
            13_000,
        ),
        Err(PersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn live_lease_blocks_unfenced_attempt_and_matching_fence_delivers() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    let fence = enqueue_and_claim(
        &mut client,
        "event_fenced_deliver",
        "worker_owner",
        "outbox_lease_owner",
        20_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_outbox_delivery_attempt(
            &mut transaction,
            identity("event_fenced_deliver"),
            "attempt_unfenced",
            DeliveryOutcome::Delivered,
            10_001,
            None,
        ),
        Err(PersistenceError::OutboxLeaseHeld)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_fenced_deliver"),
            "attempt_stale",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            fence + 1,
        ),
        Err(PersistenceError::StaleLease)
    ));
    let delivered = record_leased_outbox_delivery_attempt(
        &mut transaction,
        identity("event_fenced_deliver"),
        "attempt_owner",
        DeliveryOutcome::Delivered,
        10_001,
        None,
        fence,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(delivered.disposition(), PersistenceDisposition::Inserted);
    assert_eq!(delivered.outbox_state(), OutboxState::Delivered);
    assert_eq!(
        lease_row(&mut client, "event_fenced_deliver"),
        (None, None, "delivered".to_owned())
    );
}

#[test]
fn unexpired_lease_and_missing_or_unleased_outbox_fail_closed() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue_and_claim(
        &mut client,
        "event_still_live",
        "worker_live",
        "outbox_lease_live",
        20_000,
    );
    enqueue(&mut client, "event_never_claimed");

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_still_live"), 19_999),
        Err(PersistenceError::LeaseStillActive)
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_missing"), 20_000),
        Err(PersistenceError::OutboxNotFound)
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_never_claimed"), 20_000),
        Err(PersistenceError::NotLeased)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_still_live"),
            "worker_other",
            "outbox_lease_other",
            10_000,
            21_000,
        ),
        Err(PersistenceError::NotLeaseable)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity(" "),
            "worker_live",
            "outbox_lease_live",
            10_000,
            21_000,
        ),
        Err(PersistenceError::InvalidReference)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_never_claimed"),
            "worker_live",
            "outbox_lease_live",
            10_000,
            10_000,
        ),
        Err(PersistenceError::InvalidLeaseWindow)
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_still_live"), 0),
        Err(PersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_absent_row"),
            "worker_live",
            "outbox_lease_live",
            10_000,
            21_000,
        ),
        Err(PersistenceError::OutboxNotFound)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn fenced_attempt_validation_fails_closed() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue_and_claim(
        &mut client,
        "event_still_live",
        "worker_live",
        "outbox_lease_live",
        20_000,
    );
    enqueue(&mut client, "event_never_claimed");

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_never_claimed"),
            "attempt_unleased",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            1,
        ),
        Err(PersistenceError::NotLeased)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_still_live"),
            "attempt_zero_fence",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            0,
        ),
        Err(PersistenceError::InvalidFencingToken)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_still_live"),
            "attempt_zero_time",
            DeliveryOutcome::Delivered,
            0,
            None,
            1,
        ),
        Err(PersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_never_claimed"),
            "worker_live",
            "outbox_lease_live",
            10_000,
            u64::MAX,
        ),
        Err(PersistenceError::ValueOutOfRange)
    ));
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_never_claimed"),
            "worker_live",
            "outbox_lease_live",
            0,
            21_000,
        ),
        Err(PersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_absent_row"),
            "attempt_missing",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            1,
        ),
        Err(PersistenceError::OutboxNotFound)
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity(" "), 20_000),
        Err(PersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn terminal_outbox_is_not_leaseable_and_expired_fenced_attempt_fails_closed() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue(&mut client, "event_already_delivered");
    {
        let mut transaction = client.transaction().unwrap();
        record_outbox_delivery_attempt(
            &mut transaction,
            identity("event_already_delivered"),
            "attempt_done",
            DeliveryOutcome::Delivered,
            10_001,
            None,
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_already_delivered"),
            "worker_late",
            "outbox_lease_late",
            10_002,
            11_000,
        ),
        Err(PersistenceError::NotLeaseable)
    ));
    transaction.rollback().unwrap();

    let fence = enqueue_and_claim(
        &mut client,
        "event_lease_expired_attempt",
        "worker_slow",
        "outbox_lease_slow",
        10_500,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_lease_expired_attempt"),
            "attempt_late",
            DeliveryOutcome::Delivered,
            10_500,
            None,
            fence,
        ),
        Err(PersistenceError::LeaseExpired)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn fenced_attempt_replay_is_idempotent_and_conflicts_fail_closed() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    let fence = enqueue_and_claim(
        &mut client,
        "event_fenced_replay",
        "worker_replay",
        "outbox_lease_replay",
        20_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_fenced_replay"),
            "attempt_early",
            DeliveryOutcome::Delivered,
            9_999,
            None,
            fence,
        ),
        Err(PersistenceError::NonMonotonicTimestamp)
    ));
    let inserted = record_leased_outbox_delivery_attempt(
        &mut transaction,
        identity("event_fenced_replay"),
        "attempt_replay",
        DeliveryOutcome::Delivered,
        10_001,
        None,
        fence,
    )
    .unwrap();
    let duplicate = record_leased_outbox_delivery_attempt(
        &mut transaction,
        identity("event_fenced_replay"),
        "attempt_replay",
        DeliveryOutcome::Delivered,
        10_001,
        None,
        fence,
    )
    .unwrap();
    assert_eq!(inserted.disposition(), PersistenceDisposition::Inserted);
    assert_eq!(duplicate.disposition(), PersistenceDisposition::Duplicate);
    assert_eq!(duplicate.outbox_state(), OutboxState::Delivered);
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_fenced_replay"),
            "attempt_replay",
            DeliveryOutcome::PermanentFailure,
            10_001,
            None,
            fence,
        ),
        Err(PersistenceError::ConflictingReplay)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_fenced_replay"),
            "attempt_late_time",
            DeliveryOutcome::Delivered,
            9_999,
            None,
            fence,
        ),
        Err(PersistenceError::NotLeased)
    ));
    transaction.commit().unwrap();
}

#[test]
fn outbox_lease_operations_require_read_committed() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue(&mut client, "event_serializable");

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_serializable"),
            "worker_serializable",
            "outbox_lease_serializable",
            10_000,
            11_000,
        ),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_serializable"), 11_000),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_serializable"),
            "attempt_serializable",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            1,
        ),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_outbox_relation_is_a_database_failure() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_missing_table"),
            "worker_missing",
            "outbox_lease_missing",
            10_000,
            11_000,
        ),
        Err(PersistenceError::Database(_))
    ));
    assert!(matches!(
        expire_outbox_delivery_lease(&mut transaction, identity("event_missing_table"), 11_000),
        Err(PersistenceError::Database(_))
    ));
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_missing_table"),
            "attempt_missing",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            1,
        ),
        Err(PersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn expiry_classify_select_failure_is_a_database_failure() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue_and_claim(
        &mut client,
        "event_classify_hidden",
        "worker_hidden",
        "outbox_lease_hidden",
        11_000,
    );
    let sink = format!("outbox_lease_classify_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION outbox_lease_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER outbox_lease_redirect_after_update
             AFTER UPDATE ON integration_outbox
             FOR EACH STATEMENT EXECUTE FUNCTION outbox_lease_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        expire_outbox_delivery_lease(
            &mut transaction,
            identity("event_missing_after_update"),
            11_000
        ),
        Err(PersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn claim_classify_select_failure_is_a_database_failure() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    enqueue(&mut client, "event_claim_classify_hidden");
    let sink = format!("outbox_claim_classify_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION outbox_claim_redirect_after_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER outbox_claim_redirect_after_update
             AFTER UPDATE ON integration_outbox
             FOR EACH STATEMENT EXECUTE FUNCTION outbox_claim_redirect_after_update();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        claim_outbox_delivery(
            &mut transaction,
            identity("event_claim_missing_after_update"),
            "worker_claim_hidden",
            "outbox_lease_claim_hidden",
            10_000,
            11_000,
        ),
        Err(PersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn leased_outbox_update_failure_is_a_database_failure() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    let fence = enqueue_and_claim(
        &mut client,
        "event_lease_update_failure",
        "worker_update",
        "outbox_lease_update",
        20_000,
    );
    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION outbox_fail_lease_update()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 RAISE EXCEPTION 'forced outbox lease update failure';
             END $$;
             CREATE TRIGGER outbox_fail_lease_update
             BEFORE UPDATE ON integration_outbox
             FOR EACH ROW EXECUTE FUNCTION outbox_fail_lease_update();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        record_leased_outbox_delivery_attempt(
            &mut transaction,
            identity("event_lease_update_failure"),
            "attempt_update_failure",
            DeliveryOutcome::Delivered,
            10_001,
            None,
            fence,
        ),
        Err(PersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
