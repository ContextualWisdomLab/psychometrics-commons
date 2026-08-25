//! Regression for caller-provided item-delivery persistence aliases.
//!
//! The persistence boundary must not silently trim a tenant reference and bind
//! a padded spelling to the canonical tenant. Database constraints cannot catch
//! this defect after Rust has already normalized the value.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger, ItemDeliveryPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

static EXACT_REFERENCE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    EXACT_REFERENCE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS item_delivery_exact_reference_test;\
             SET search_path TO item_delivery_exact_reference_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS item_delivery_exact_reference_test.item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_exact_reference_test.item_delivery_ledger;",
        )
        .unwrap();
}

fn ledger() -> ItemDeliveryLedger {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_version_big_five_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    ItemDeliveryLedger::from_manifest("session_item_delivery_exact_ref", &manifest).unwrap()
}

#[test]
fn padded_tenant_aliases_fail_before_any_item_delivery_row_is_written() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = ledger();

    for invalid in [
        " tenant_item_delivery_alpha",
        "tenant_item_delivery_alpha ",
        "\u{00a0}tenant_item_delivery_alpha",
    ] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_item_delivery_ledger(&mut transaction, invalid, &ledger),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ));
        let row = transaction
            .query_one("SELECT COUNT(*) FROM item_delivery_ledger", &[])
            .unwrap();
        assert_eq!(row.get::<_, i64>(0), 0);
        transaction.rollback().unwrap();
    }
}
