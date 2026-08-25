//! Durable response-snapshot references must match the Rust opaque-reference boundary.
//!
//! Response collection normalizes Unicode whitespace and rejects embedded control characters and
//! numeric-like spellings under Rust `char::is_numeric`. Direct SQL and migration reapplication
//! must not persist response identities the domain would reject or normalize.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::apply_response_snapshot_migration;
use std::sync::{Mutex, MutexGuard};

const DATABASE_TEST_LOCK_KEY: i64 = 8_139_518_222_897_414_902;
const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS response_snapshot_reference_parity_test CASCADE; \
             CREATE SCHEMA response_snapshot_reference_parity_test; \
             SET search_path TO response_snapshot_reference_parity_test;",
        )
        .unwrap();
    apply_response_snapshot_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_header(
    client: &mut Client,
    snapshot_ref: &str,
    session_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO response_snapshot (snapshot_ref, session_ref, event_count, last_sequence) \
         VALUES ($1,$2,1,1)",
        &[&snapshot_ref, &session_ref],
    )
}

fn insert_entry(
    client: &mut Client,
    snapshot_ref: &str,
    event_ref: &str,
    item_version_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO response_snapshot_entry (\
             snapshot_ref, snapshot_sequence, event_ref, item_version_ref, payload_digest\
         ) VALUES ($1,1,$2,$3,$4)",
        &[&snapshot_ref, &event_ref, &item_version_ref, &VALID_DIGEST],
    )
}

#[test]
fn fixture_lock_is_visible_to_another_postgres_session() {
    let _guard = guard();
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut contender = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .unwrap()
        .get(0);
    if acquired {
        contender
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&DATABASE_TEST_LOCK_KEY],
            )
            .unwrap();
    }
    assert!(
        !acquired,
        "fixture serialization must be visible across PostgreSQL sessions"
    );
}

#[test]
fn snapshot_and_session_references_reject_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_header(
            &mut client,
            invalid_ref,
            &format!("session_snapshot_ref_{index}"),
        )
        .expect_err("snapshot references must match the Rust opaque-reference boundary");
        assert_check(&error, "response_snapshot_snapshot_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_header(
            &mut client,
            &format!("response_snapshot_session_ref_{index}"),
            invalid_ref,
        )
        .expect_err("session references must match the Rust opaque-reference boundary");
        assert_check(&error, "response_snapshot_session_ref_format_check");
    }
}

#[test]
fn entry_event_and_item_references_reject_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let snapshot_ref = format!("response_snapshot_event_ref_{index}");
        insert_header(
            &mut client,
            &snapshot_ref,
            &format!("session_event_ref_{index}"),
        )
        .unwrap();
        let error = insert_entry(
            &mut client,
            &snapshot_ref,
            invalid_ref,
            &format!("item_version_event_ref_{index}"),
        )
        .expect_err("event references must match the Rust opaque-reference boundary");
        assert_check(&error, "response_snapshot_entry_event_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let snapshot_ref = format!("response_snapshot_item_ref_{index}");
        insert_header(
            &mut client,
            &snapshot_ref,
            &format!("session_item_ref_{index}"),
        )
        .unwrap();
        let error = insert_entry(
            &mut client,
            &snapshot_ref,
            &format!("response_event_item_ref_{index}"),
            invalid_ref,
        )
        .expect_err("item-version references must match the Rust opaque-reference boundary");
        assert_check(
            &error,
            "response_snapshot_entry_item_version_ref_format_check",
        );
    }
}

#[test]
fn migration_reapplication_replaces_a_weakened_snapshot_reference_constraint() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE response_snapshot \
                 DROP CONSTRAINT response_snapshot_snapshot_ref_format_check; \
             ALTER TABLE response_snapshot \
                 ADD CONSTRAINT response_snapshot_snapshot_ref_format_check CHECK (\
                     snapshot_ref = btrim(snapshot_ref) \
                     AND snapshot_ref <> '' \
                     AND NOT (\
                         snapshot_ref ~ '[[:digit:]]' \
                         AND snapshot_ref ~ '^[[:digit:]+,.eE-]+$'\
                     )\
                 );",
        )
        .unwrap();

    apply_response_snapshot_migration(&mut client).unwrap();

    let error = insert_header(&mut client, "½", "session_upgrade_reference_guard")
        .expect_err("migration reapplication must repair the weaker reference constraint");
    assert_check(&error, "response_snapshot_snapshot_ref_format_check");
}
