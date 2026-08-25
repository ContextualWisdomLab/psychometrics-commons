//! Durable response snapshots must preserve the domain's canonical payload-digest contract.
//!
//! `ResponseLedger` accepts only exact lowercase `sha256:` evidence with 64 hexadecimal digits.
//! Direct SQL and migration reapplication must not persist a weaker payload identity that the
//! product domain could never produce.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::apply_response_snapshot_migration;

const DATABASE_TEST_LOCK_KEY: i64 = 8_139_518_222_897_414_901;
const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn guard() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut guard = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    guard
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("fixture must acquire the database-visible advisory lock");
    guard
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS response_snapshot_digest_integrity_test CASCADE; \
             CREATE SCHEMA response_snapshot_digest_integrity_test; \
             SET search_path TO response_snapshot_digest_integrity_test;",
        )
        .unwrap();
    apply_response_snapshot_migration(&mut client).unwrap();
    client
}

fn insert_entry(
    client: &mut Client,
    suffix: &str,
    payload_digest: &str,
) -> Result<u64, postgres::Error> {
    let snapshot_ref = format!("response_snapshot_digest_{suffix}");
    let session_ref = format!("session_digest_{suffix}");
    let event_ref = format!("response_event_digest_{suffix}");
    let item_version_ref = format!("item_version_digest_{suffix}");

    client.execute(
        "INSERT INTO response_snapshot (snapshot_ref, session_ref, event_count, last_sequence) \
         VALUES ($1,$2,1,1)",
        &[&snapshot_ref, &session_ref],
    )?;
    client.execute(
        "INSERT INTO response_snapshot_entry (\
             snapshot_ref, snapshot_sequence, event_ref, item_version_ref, payload_digest\
         ) VALUES ($1,1,$2,$3,$4)",
        &[
            &snapshot_ref,
            &event_ref,
            &item_version_ref,
            &payload_digest,
        ],
    )
}

fn assert_digest_check(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("payload rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database_error.constraint(),
        Some("response_snapshot_entry_payload_digest_format_check")
    );
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
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .unwrap();
    }
    assert!(
        !acquired,
        "fixture serialization must be visible across PostgreSQL sessions"
    );
}

#[test]
fn direct_sql_rejects_every_noncanonical_payload_digest_shape() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_digest) in [
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef",
        " sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef ",
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
    ]
    .into_iter()
    .enumerate()
    {
        let error = insert_entry(&mut client, &format!("invalid_{index}"), invalid_digest)
            .expect_err("direct SQL must preserve the canonical response payload digest boundary");
        assert_digest_check(&error);
    }
}

#[test]
fn canonical_lowercase_sha256_digest_remains_persistable() {
    let _guard = guard();
    let mut client = client();

    assert_eq!(insert_entry(&mut client, "valid", VALID_DIGEST).unwrap(), 1);
}

#[test]
fn migration_reapplication_replaces_the_weaker_not_blank_digest_constraint() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE response_snapshot_entry \
                 DROP CONSTRAINT response_snapshot_entry_payload_digest_format_check; \
             ALTER TABLE response_snapshot_entry \
                 ADD CONSTRAINT response_snapshot_entry_payload_digest_not_blank_check CHECK (\
                     payload_digest = btrim(payload_digest) AND payload_digest <> ''\
                 );",
        )
        .unwrap();

    apply_response_snapshot_migration(&mut client).unwrap();

    let error = insert_entry(
        &mut client,
        "upgrade_guard",
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .expect_err("migration reapplication must replace the historical not-blank digest guard");
    assert_digest_check(&error);
}
