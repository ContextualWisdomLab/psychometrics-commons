//! Tenant-bound adapter contract for item-delivery persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::ItemDeliveryLedger;
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
static TENANT_ADAPTER_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    TENANT_ADAPTER_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database")
}

fn test_client() -> Client {
    let mut client = Client::connect(&database_url(), NoTls)
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

fn peer_client() -> Client {
    let mut client = Client::connect(&database_url(), NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute("SET search_path TO item_delivery_tenant_adapter_test")
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
fn concurrent_exact_header_replay_is_duplicate_after_winning_insert_commits() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_item_delivery_migration(&mut client).unwrap();
    let mut peer = peer_client();
    let peer_pid: i32 = peer.query_one("SELECT pg_backend_pid()", &[]).unwrap().get(0);
    let ledger = ItemDeliveryLedger::from_manifest("session_concurrent_header", &manifest()).unwrap();

    let mut winner = client.transaction().unwrap();
    assert_eq!(
        persist_item_delivery_ledger(&mut winner, "tenant_alpha", &ledger).unwrap(),
        ItemDeliveryPersistenceDisposition::Inserted
    );

    let (started_tx, started_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let ledger =
            ItemDeliveryLedger::from_manifest("session_concurrent_header", &manifest()).unwrap();
        let mut transaction = peer.transaction().unwrap();
        started_tx.send(()).unwrap();
        let result = persist_item_delivery_ledger(&mut transaction, "tenant_alpha", &ledger);
        match &result {
            Ok(_) => transaction.commit().unwrap(),
            Err(_) => transaction.rollback().unwrap(),
        }
        result
    });
    started_rx.recv().unwrap();

    let mut observed_block = false;
    for _ in 0..100 {
        observed_block = winner
            .query_one(
                "SELECT cardinality(pg_blocking_pids($1)) > 0",
                &[&peer_pid],
            )
            .unwrap()
            .get(0);
        if observed_block {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(observed_block, "the replay must reach the unique-key wait before the winner commits");
    winner.commit().unwrap();

    assert_eq!(
        waiter.join().unwrap().unwrap(),
        ItemDeliveryPersistenceDisposition::Duplicate,
        "READ COMMITTED exact replay must classify the just-committed winning header in a fresh statement snapshot"
    );
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

#[test]
fn padded_tenant_alias_fails_closed_on_persist() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger =
        ItemDeliveryLedger::from_manifest("session_padded_persist_tenant", &manifest()).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            persist_item_delivery_ledger(&mut transaction, " tenant_alpha", &ledger),
            Err(ItemDeliveryPersistenceError::InvalidReference)
        ),
        "a padded persist tenant must not be stored as the trimmed tenant identity"
    );
    transaction.rollback().unwrap();
}
