//! Tenant-bound adapter contract for item-delivery persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const DATABASE_TEST_LOCK_KEY: i64 = 0x4954_444C_5652_4C4B;
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
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

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_big_five_ko_v1"),
        "narrative_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        RELEASE_DIGEST,
    )
    .unwrap()
}

#[test]
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let _guard = test_guard();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one("SELECT pg_try_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("cross-process fixture lock should be observable from PostgreSQL")
        .get(0);
    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .expect("RED fixture lock should be released after probing");
    }
    assert!(
        !acquired,
        "a process-local mutex cannot serialize a fixed PostgreSQL schema across CI processes"
    );
}

#[test]
fn adapter_requires_and_persists_explicit_tenant_scope() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = ItemDeliveryLedger::from_manifest("session_tenant_bound", &manifest()).unwrap();

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
    let ledger = ItemDeliveryLedger::from_manifest("session_numeric_tenant", &manifest()).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, "123", &ledger),
        Err(ItemDeliveryPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}
