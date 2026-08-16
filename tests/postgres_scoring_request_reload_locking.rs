//! Real `PostgreSQL` locking evidence for scoring-request reload.
//!
//! `load_scoring_request` takes `FOR SHARE` on the unique request row. A
//! concurrent writer that tries to rewrite that identity must wait until the
//! caller-owned reload transaction ends. In-memory mutex behavior is not
//! evidence (ADR-0015 concurrency invariant 5).

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, load_scoring_request, persist_scoring_request,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "scoring_request_reload_locking_test";
const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENGINE_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
static DATABASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_clients() -> (MutexGuard<'static, ()>, Client, Client) {
    let guard = DATABASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut owner = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    owner
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    contender
        .batch_execute(&format!("SET search_path TO {SCHEMA};"))
        .unwrap();
    (guard, owner, contender)
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(&format!("DROP TABLE IF EXISTS {SCHEMA}.scoring_request;"))
        .unwrap();
}

fn persist_two_item_request(client: &mut Client, scoring_request_ref: &str) {
    let mut ledger = ResponseLedger::new("session_reload_lock").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_zzz_first",
                client_event_ref: "client_event_001",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            },
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_aaa_second",
                client_event_ref: "client_event_002",
                item_version_ref: "item_version_002",
                payload_digest: ENGINE_DIGEST,
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_reload_lock")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: "response_snapshot_reload_lock",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_scoring_request(&mut transaction, &request).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn scoring_request_reload_share_lock_blocks_concurrent_rewrite() {
    let (_guard, mut owner, mut contender) = test_clients();
    reset_tables(&mut owner);
    apply_scoring_request_migration(&mut owner).unwrap();
    persist_two_item_request(&mut owner, "scoring_request_reload_lock");

    let mut transaction = owner.transaction().unwrap();
    let loaded = load_scoring_request(&mut transaction, "scoring_request_reload_lock")
        .expect("reload must succeed while the share lock is held")
        .expect("the persisted request must still be present");
    assert_eq!(loaded.scoring_request_ref(), "scoring_request_reload_lock");

    contender
        .batch_execute("SET lock_timeout = '100ms';")
        .unwrap();
    let error = contender
        .execute(
            "UPDATE scoring_request
             SET created_at = created_at
             WHERE scoring_request_ref = $1",
            &[&"scoring_request_reload_lock"],
        )
        .expect_err("reload FOR SHARE must keep a concurrent rewrite waiting");
    assert_eq!(
        error.code().map(postgres::error::SqlState::code),
        Some("55P03")
    );

    transaction.rollback().unwrap();
}
