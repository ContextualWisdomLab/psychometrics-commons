//! Real `PostgreSQL` contract for inbox consumption as distinct from receipt.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, complete_inbox_consumption,
    persist_inbox_consumption, quarantine_inbox_consumption, InboxConsumptionDisposition,
    InboxConsumptionPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

static INBOX_CONSUMPTION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    INBOX_CONSUMPTION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS inbox_consumption_persistence_test;\
             SET search_path TO inbox_consumption_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS inbox_consumption_persistence_test.integration_consumption;\
             DROP TABLE IF EXISTS inbox_consumption_persistence_test.integration_inbox;\
             DROP TABLE IF EXISTS inbox_consumption_persistence_test.integration_delivery_attempt;\
             DROP TABLE IF EXISTS inbox_consumption_persistence_test.integration_outbox;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_inbox_consumption_migration(client).unwrap();
}

fn source_event(event_ref: &str) -> IntegrationEvent {
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

fn accept_inbox(client: &mut Client, event_ref: &str) {
    accept_inbox_event(client, "consumer_alpha", &source_event(event_ref), 20_000).unwrap();
}

fn pending(event_ref: &str, consumption_ref: &str, side_effect_ref: &str) -> InboxConsumption {
    InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        event_ref,
        consumption_ref,
        side_effect_ref,
        20_000,
    )
    .unwrap()
}

fn persist_ok(client: &mut Client, consumption: &InboxConsumption) -> InboxConsumptionDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_inbox_consumption(&mut transaction, consumption).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    consumption: &InboxConsumption,
) -> InboxConsumptionPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_inbox_consumption(&mut transaction, consumption).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn stored_state(client: &mut Client, event_ref: &str, consumption_ref: &str) -> (String, i64) {
    let row = client
        .query_one(
            "SELECT consumption_state, fencing_token FROM integration_consumption \
             WHERE consumer_ref = 'consumer_alpha' AND source_ref = 'psychometrics_commons' \
               AND tenant_ref = 'tenant_alpha' AND source_event_ref = $1 \
               AND consumption_ref = $2",
            &[&event_ref, &consumption_ref],
        )
        .unwrap();
    (row.get(0), row.get(1))
}

#[test]
fn receipt_without_consumption_is_not_completion() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_receipt_only");

    let completed: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_consumption \
             WHERE source_event_ref = 'event_receipt_only' \
               AND consumption_state = 'completed'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(completed, 0);
}

#[test]
fn persist_is_idempotent_and_requires_inbox_receipt() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    let consumption = pending(
        "event_persist",
        "consumption_persist",
        "side_effect_projection",
    );
    assert!(matches!(
        persist_err(&mut client, &consumption),
        InboxConsumptionPersistenceError::InboxNotFound
    ));

    accept_inbox(&mut client, "event_persist");
    assert_eq!(
        persist_ok(&mut client, &consumption),
        InboxConsumptionDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &consumption),
        InboxConsumptionDisposition::Duplicate
    );
    assert_eq!(
        stored_state(&mut client, "event_persist", "consumption_persist"),
        ("pending".to_owned(), 0)
    );
}

#[test]
fn persist_rebinding_and_non_fresh_state_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_rebind");
    persist_ok(
        &mut client,
        &pending(
            "event_rebind",
            "consumption_rebind",
            "side_effect_projection",
        ),
    );

    let rebound = pending(
        "event_rebind",
        "consumption_rebind",
        "side_effect_other_work",
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        InboxConsumptionPersistenceError::ConflictingReplay
    ));

    let same_side_effect = pending(
        "event_rebind",
        "consumption_other_work",
        "side_effect_projection",
    );
    assert!(matches!(
        persist_err(&mut client, &same_side_effect),
        InboxConsumptionPersistenceError::ConflictingReplay
    ));

    let mut completed = pending(
        "event_rebind",
        "consumption_completed_shape",
        "side_effect_completed_shape",
    );
    completed
        .complete(20_001, "completion_projection_applied", 0)
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &completed),
        InboxConsumptionPersistenceError::UnsupportedInitialState
    ));

    let later = InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_rebind",
        "consumption_rebind",
        "side_effect_projection",
        20_500,
    )
    .unwrap();
    assert!(matches!(
        persist_err(&mut client, &later),
        InboxConsumptionPersistenceError::ConflictingReplay
    ));

    let claimed = pending(
        "event_rebind",
        "consumption_claimed_shape",
        "side_effect_claimed_shape",
    );
    persist_ok(&mut client, &claimed);
    let mut claim = client.transaction().unwrap();
    begin_inbox_consumption(&mut claim, &claimed, 20_001).unwrap();
    claim.commit().unwrap();
    assert!(matches!(
        persist_err(&mut client, &claimed),
        InboxConsumptionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn local_complete_and_claimed_complete_are_fenced() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_complete");
    let local = pending("event_complete", "consumption_local", "side_effect_local");
    persist_ok(&mut client, &local);
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        complete_inbox_consumption(
            &mut transaction,
            &local,
            20_001,
            "completion_projection_applied",
            0,
        )
        .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    assert_eq!(
        complete_inbox_consumption(
            &mut transaction,
            &local,
            20_001,
            "completion_projection_applied",
            0,
        )
        .unwrap(),
        InboxConsumptionDisposition::Duplicate
    );
    transaction.commit().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_complete", "consumption_local"),
        ("completed".to_owned(), 0)
    );

    let claimed = pending(
        "event_complete",
        "consumption_claimed",
        "side_effect_claimed",
    );
    persist_ok(&mut client, &claimed);
    let mut claim = client.transaction().unwrap();
    assert_eq!(
        begin_inbox_consumption(&mut claim, &claimed, 20_001).unwrap(),
        1
    );
    assert!(matches!(
        begin_inbox_consumption(&mut claim, &claimed, 20_002),
        Err(InboxConsumptionPersistenceError::ConsumptionNotClaimable)
    ));
    assert!(matches!(
        complete_inbox_consumption(
            &mut claim,
            &claimed,
            20_002,
            "completion_projection_applied",
            0,
        ),
        Err(InboxConsumptionPersistenceError::StaleConsumptionFence)
    ));
    assert_eq!(
        complete_inbox_consumption(
            &mut claim,
            &claimed,
            20_002,
            "completion_projection_applied",
            1,
        )
        .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    claim.commit().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_complete", "consumption_claimed"),
        ("completed".to_owned(), 1)
    );
}

#[test]
fn quarantine_and_terminal_transitions_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_quarantine");
    let consumption = pending(
        "event_quarantine",
        "consumption_quarantine",
        "side_effect_quarantine",
    );
    persist_ok(&mut client, &consumption);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        quarantine_inbox_consumption(&mut transaction, &consumption, 20_001, "poison_payload", 0)
            .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    assert_eq!(
        quarantine_inbox_consumption(&mut transaction, &consumption, 20_001, "poison_payload", 0)
            .unwrap(),
        InboxConsumptionDisposition::Duplicate
    );
    assert!(matches!(
        quarantine_inbox_consumption(&mut transaction, &consumption, 20_001, "other_cause", 0),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    assert!(matches!(
        complete_inbox_consumption(
            &mut transaction,
            &consumption,
            20_002,
            "completion_projection_applied",
            0,
        ),
        Err(InboxConsumptionPersistenceError::TerminalConsumptionState)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut transaction, &consumption, 20_002),
        Err(InboxConsumptionPersistenceError::TerminalConsumptionState)
    ));
    transaction.commit().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_quarantine", "consumption_quarantine"),
        ("quarantined".to_owned(), 0)
    );

    let completed = pending(
        "event_quarantine",
        "consumption_already_complete",
        "side_effect_already_complete",
    );
    persist_ok(&mut client, &completed);
    let mut complete = client.transaction().unwrap();
    complete_inbox_consumption(
        &mut complete,
        &completed,
        20_001,
        "completion_projection_applied",
        0,
    )
    .unwrap();
    assert!(matches!(
        quarantine_inbox_consumption(&mut complete, &completed, 20_002, "poison_payload", 0),
        Err(InboxConsumptionPersistenceError::TerminalConsumptionState)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut complete, &completed, 20_002),
        Err(InboxConsumptionPersistenceError::TerminalConsumptionState)
    ));
    assert!(matches!(
        complete_inbox_consumption(
            &mut complete,
            &completed,
            20_001,
            "completion_other_evidence",
            0,
        ),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    complete.commit().unwrap();
}

#[test]
fn invalid_inputs_isolation_and_missing_rows_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_validation");
    let consumption = pending(
        "event_validation",
        "consumption_validation",
        "side_effect_validation",
    );
    persist_ok(&mut client, &consumption);
    let missing = pending(
        "event_validation",
        "consumption_missing",
        "side_effect_missing",
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        begin_inbox_consumption(&mut transaction, &consumption, 0),
        Err(InboxConsumptionPersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut transaction, &consumption, 19_999),
        Err(InboxConsumptionPersistenceError::NonMonotonicTimestamp)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut transaction, &missing, 20_001),
        Err(InboxConsumptionPersistenceError::ConsumptionNotFound)
    ));
    assert!(matches!(
        complete_inbox_consumption(&mut transaction, &consumption, 0, "completion_ok", 0),
        Err(InboxConsumptionPersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        complete_inbox_consumption(&mut transaction, &consumption, 20_001, "12345", 0),
        Err(InboxConsumptionPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        complete_inbox_consumption(&mut transaction, &consumption, 19_999, "completion_ok", 0,),
        Err(InboxConsumptionPersistenceError::NonMonotonicTimestamp)
    ));
    assert!(matches!(
        complete_inbox_consumption(&mut transaction, &consumption, 20_001, "completion_ok", 1),
        Err(InboxConsumptionPersistenceError::StaleConsumptionFence)
    ));
    assert!(matches!(
        quarantine_inbox_consumption(&mut transaction, &consumption, 20_001, "   ", 0),
        Err(InboxConsumptionPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_inbox_consumption(&mut serializable, &consumption),
        Err(InboxConsumptionPersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut serializable, &consumption, 20_001),
        Err(InboxConsumptionPersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        complete_inbox_consumption(&mut serializable, &consumption, 20_001, "completion_ok", 0,),
        Err(InboxConsumptionPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut overflow = client.transaction().unwrap();
    assert!(matches!(
        complete_inbox_consumption(&mut overflow, &consumption, u64::MAX, "completion_ok", 0,),
        Err(InboxConsumptionPersistenceError::ValueOutOfRange)
    ));
    assert!(matches!(
        begin_inbox_consumption(&mut overflow, &consumption, u64::MAX),
        Err(InboxConsumptionPersistenceError::ValueOutOfRange)
    ));
    overflow.rollback().unwrap();
}

#[test]
fn claimed_quarantine_and_overflow_persist_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_claimed_validation");
    let consumption = pending(
        "event_claimed_validation",
        "consumption_claimed_validation",
        "side_effect_claimed_validation",
    );
    persist_ok(&mut client, &consumption);

    let overflow_pending = InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_claimed_validation",
        "consumption_overflow",
        "side_effect_overflow",
        u64::MAX,
    )
    .unwrap();
    assert!(matches!(
        persist_err(&mut client, &overflow_pending),
        InboxConsumptionPersistenceError::ValueOutOfRange
    ));

    let mut claimed = client.transaction().unwrap();
    begin_inbox_consumption(&mut claimed, &consumption, 20_001).unwrap();
    assert!(matches!(
        quarantine_inbox_consumption(&mut claimed, &consumption, 20_000, "poison_payload", 1),
        Err(InboxConsumptionPersistenceError::NonMonotonicTimestamp)
    ));
    assert!(matches!(
        quarantine_inbox_consumption(&mut claimed, &consumption, 20_002, "poison_payload", 0),
        Err(InboxConsumptionPersistenceError::StaleConsumptionFence)
    ));
    assert_eq!(
        quarantine_inbox_consumption(&mut claimed, &consumption, 20_002, "poison_payload", 1)
            .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    claimed.commit().unwrap();
}

#[test]
fn processing_row_cannot_be_stolen_and_crash_leaves_recoverable_state() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_crash");
    let consumption = pending("event_crash", "consumption_crash", "side_effect_crash");
    persist_ok(&mut client, &consumption);

    let mut worker = client.transaction().unwrap();
    assert_eq!(
        begin_inbox_consumption(&mut worker, &consumption, 20_001).unwrap(),
        1
    );
    worker.commit().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_crash", "consumption_crash"),
        ("processing".to_owned(), 1)
    );

    let mut thief = client.transaction().unwrap();
    assert!(matches!(
        begin_inbox_consumption(&mut thief, &consumption, 20_002),
        Err(InboxConsumptionPersistenceError::ConsumptionNotClaimable)
    ));
    thief.rollback().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_crash", "consumption_crash"),
        ("processing".to_owned(), 1)
    );

    let mut recovered = client.transaction().unwrap();
    assert_eq!(
        complete_inbox_consumption(
            &mut recovered,
            &consumption,
            20_003,
            "completion_after_retry",
            1,
        )
        .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    recovered.commit().unwrap();
    assert_eq!(
        stored_state(&mut client, "event_crash", "consumption_crash"),
        ("completed".to_owned(), 1)
    );
}

#[test]
fn database_failures_preserve_source() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    accept_inbox(&mut client, "event_db");
    let consumption = pending("event_db", "consumption_db", "side_effect_db");
    client
        .batch_execute("DROP TABLE integration_consumption;")
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_inbox_consumption(&mut transaction, &consumption).unwrap_err();
    assert!(matches!(
        error,
        InboxConsumptionPersistenceError::Database(_)
    ));
    assert!(std::error::Error::source(&error).is_some());
    transaction.rollback().unwrap();
}
