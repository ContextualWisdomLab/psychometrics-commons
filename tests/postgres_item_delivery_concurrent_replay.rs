//! Real PostgreSQL regressions for exact item-delivery replay under concurrent commits.
//!
//! PostgreSQL Read Committed gives each command a fresh snapshot, while `INSERT ... ON CONFLICT
//! DO NOTHING` can observe a unique conflict whose row is not visible to that same command's
//! snapshot. Exact replay must therefore retry the read in a later command instead of turning a
//! concurrent duplicate into an opaque database failure.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const SCHEMA: &str = "item_delivery_concurrent_replay_test";
const TENANT_REF: &str = "tenant_item_delivery_concurrent";
const DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn connection_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database")
}

fn client(connection: &str) -> Client {
    let mut client = Client::connect(connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!("SET search_path TO {SCHEMA};"))
        .unwrap();
    client
}

fn reset_schema(connection: &str) -> Client {
    let mut client = Client::connect(connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_item_delivery_concurrent",
        "instrument_big_five",
        "instrument_version_concurrent",
        "construct_big_five",
        &["item_version_concurrent"],
        "en-US",
        "assessment_spec_concurrent",
        "scoring_concurrent",
        "calibration_concurrent",
        Some("norm_concurrent"),
        "narrative_concurrent",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        DIGEST,
    )
    .unwrap()
}

fn empty_ledger(session_ref: &str) -> ItemDeliveryLedger {
    ItemDeliveryLedger::from_manifest(session_ref, &manifest()).unwrap()
}

fn delivered_ledger(session_ref: &str) -> ItemDeliveryLedger {
    let mut ledger = empty_ledger(session_ref);
    ledger
        .deliver(
            SessionState::Active,
            ItemDeliveryRequest {
                delivery_ref: "delivery_event_concurrent",
                item_version_ref: "item_version_concurrent",
                presentation_context_ref: "presentation_concurrent",
                selection_evidence_ref: Some("selection_concurrent"),
            },
        )
        .unwrap();
    ledger
}

fn wait_until_blocked(control: &mut Client, backend_pid: i32) {
    for _ in 0..200 {
        let blocked: bool = control
            .query_one(
                "SELECT cardinality(pg_blocking_pids($1)) > 0",
                &[&backend_pid],
            )
            .unwrap()
            .get(0);
        if blocked {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("concurrent replay did not block on the uncommitted unique conflict");
}

#[test]
fn concurrent_header_exact_replay_is_duplicate_not_database_failure() {
    let connection = connection_url();
    let mut control = reset_schema(&connection);
    let session_ref = "session_item_delivery_concurrent_header";

    let (inserted_tx, inserted_rx) = mpsc::channel();
    let (commit_tx, commit_rx) = mpsc::channel();
    let first_connection = connection.clone();
    let first = thread::spawn(move || {
        let mut client = client(&first_connection);
        let ledger = empty_ledger(session_ref);
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, TENANT_REF, &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        inserted_tx.send(()).unwrap();
        commit_rx.recv().unwrap();
        transaction.commit().unwrap();
    });
    inserted_rx.recv().unwrap();

    let (pid_tx, pid_rx) = mpsc::channel();
    let second_connection = connection.clone();
    let second = thread::spawn(move || {
        let mut client = client(&second_connection);
        let backend_pid: i32 = client
            .query_one("SELECT pg_backend_pid()", &[])
            .unwrap()
            .get(0);
        pid_tx.send(backend_pid).unwrap();
        let ledger = empty_ledger(session_ref);
        let mut transaction = client.transaction().unwrap();
        let result = persist_item_delivery_ledger(&mut transaction, TENANT_REF, &ledger)
            .map_err(|error| format!("{error:?}"));
        match result {
            Ok(disposition) => {
                transaction.commit().unwrap();
                Ok(disposition)
            }
            Err(error) => {
                transaction.rollback().unwrap();
                Err(error)
            }
        }
    });

    let second_pid = pid_rx.recv().unwrap();
    wait_until_blocked(&mut control, second_pid);
    commit_tx.send(()).unwrap();
    first.join().unwrap();

    assert_eq!(
        second.join().unwrap().unwrap(),
        ItemDeliveryPersistenceDisposition::Duplicate
    );
}

#[test]
fn concurrent_event_exact_replay_is_duplicate_not_database_failure() {
    let connection = connection_url();
    let mut control = reset_schema(&connection);
    let session_ref = "session_item_delivery_concurrent_event";
    let ledger = empty_ledger(session_ref);
    {
        let mut transaction = control.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, TENANT_REF, &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let (inserted_tx, inserted_rx) = mpsc::channel();
    let (commit_tx, commit_rx) = mpsc::channel();
    let first_connection = connection.clone();
    let first = thread::spawn(move || {
        let mut client = client(&first_connection);
        let mut transaction = client.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO item_delivery_event (tenant_ref, session_ref, delivery_event_ref, \
                 item_version_ref, presentation_context_ref, selection_evidence_ref, delivery_sequence) \
                 VALUES ($1, $2, 'delivery_event_concurrent', 'item_version_concurrent', \
                 'presentation_concurrent', 'selection_concurrent', 1)",
                &[&TENANT_REF, &session_ref],
            )
            .unwrap();
        inserted_tx.send(()).unwrap();
        commit_rx.recv().unwrap();
        transaction.commit().unwrap();
    });
    inserted_rx.recv().unwrap();

    let (pid_tx, pid_rx) = mpsc::channel();
    let second_connection = connection.clone();
    let second = thread::spawn(move || {
        let mut client = client(&second_connection);
        let backend_pid: i32 = client
            .query_one("SELECT pg_backend_pid()", &[])
            .unwrap()
            .get(0);
        pid_tx.send(backend_pid).unwrap();
        let ledger = delivered_ledger(session_ref);
        let mut transaction = client.transaction().unwrap();
        let result = persist_item_delivery_ledger(&mut transaction, TENANT_REF, &ledger)
            .map_err(|error| format!("{error:?}"));
        match result {
            Ok(disposition) => {
                transaction.commit().unwrap();
                Ok(disposition)
            }
            Err(error) => {
                transaction.rollback().unwrap();
                Err(error)
            }
        }
    });

    let second_pid = pid_rx.recv().unwrap();
    wait_until_blocked(&mut control, second_pid);
    commit_tx.send(()).unwrap();
    first.join().unwrap();

    assert_eq!(
        second.join().unwrap().unwrap(),
        ItemDeliveryPersistenceDisposition::Duplicate
    );
}
