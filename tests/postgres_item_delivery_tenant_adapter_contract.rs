//! Tenant-bound adapter contract for item-delivery persistence.

mod item_delivery_support;

use item_delivery_support::{published_release, session_with_ref_in_state};
use postgres::{Client, NoTls};
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

static TENANT_ADAPTER_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    TENANT_ADAPTER_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS item_delivery_tenant_adapter_test;\
             SET search_path TO item_delivery_tenant_adapter_test;\
             DROP TABLE IF EXISTS item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_ledger;",
        )
        .unwrap();
    client
}

fn empty_ledger(session_ref: &str) -> ItemDeliveryLedger {
    let release = published_release();
    let session = session_with_ref_in_state(&release, session_ref, SessionState::Active);
    ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap()
}

#[test]
fn adapter_requires_and_persists_explicit_tenant_scope() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = empty_ledger("session_tenant_bound");

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, "tenant_alpha", &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT tenant_ref FROM item_delivery_ledger WHERE session_ref = $1",
            &[&ledger.session_ref()],
        )
        .unwrap();
    let tenant_ref: String = row.get(0);
    assert_eq!(tenant_ref, "tenant_alpha");

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, "tenant_beta", &ledger),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn numeric_tenant_reference_fails_closed_before_write() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = empty_ledger("session_numeric_tenant");

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, "123", &ledger),
        Err(ItemDeliveryPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}
