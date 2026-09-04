//! Corruption regression for fail-closed item-delivery replay classification.

mod item_delivery_support;

use item_delivery_support::{published_release, session_with_ref_in_state};
use postgres::{Client, NoTls};
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_corrupt_tenant_test CASCADE;\
             CREATE SCHEMA item_delivery_corrupt_tenant_test;\
             SET search_path TO item_delivery_corrupt_tenant_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn delivered_ledger() -> ItemDeliveryLedger {
    let release = published_release();
    let session = session_with_ref_in_state(
        &release,
        "session_corrupt_tenant_replay",
        SessionState::Active,
    );
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    ledger
        .deliver(
            &session,
            ItemDeliveryRequest {
                delivery_ref: "delivery_corrupt_tenant_replay",
                item_version_ref: "item_version_001",
                presentation_context_ref: "presentation_standard_v1",
                selection_evidence_ref: None,
            },
        )
        .unwrap();
    ledger
}

#[test]
fn replay_fails_closed_when_stored_event_tenant_no_longer_matches_ledger() {
    let mut client = test_client();
    let ledger = delivered_ledger();

    {
        let mut transaction = client.transaction().unwrap();
        persist_item_delivery_ledger(&mut transaction, "tenant_original", &ledger).unwrap();
        transaction.commit().unwrap();
    }

    client
        .batch_execute(
            "ALTER TABLE item_delivery_event \
                 DROP CONSTRAINT item_delivery_event_session_tenant_fk;\
             UPDATE item_delivery_event \
                 SET tenant_ref = 'tenant_corrupt' \
                 WHERE session_ref = 'session_corrupt_tenant_replay' \
                   AND delivery_event_ref = 'delivery_corrupt_tenant_replay';",
        )
        .unwrap();

    let mut replay = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut replay, "tenant_original", &ledger),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    replay.rollback().unwrap();
}
