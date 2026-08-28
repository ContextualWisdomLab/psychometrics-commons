//! Response-snapshot persistence must reject the same invisible identifier aliases as Rust.
//!
//! `normalized_reference` rejects Unicode 17 Default_Ignorable_Code_Point characters because
//! byte-distinct invisible aliases are unsafe for authorization, replay, audit, and participant
//! artifacts. This real-PostgreSQL contract keeps the persisted snapshot identity boundary aligned.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_response_snapshot::apply_response_snapshot_migration;

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS response_snapshot_default_ignorable_parity_test CASCADE; \
             CREATE SCHEMA response_snapshot_default_ignorable_parity_test; \
             SET search_path TO response_snapshot_default_ignorable_parity_test;",
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
fn persisted_snapshot_references_reject_default_ignorable_aliases() {
    let mut client = client();
    let invalid_references = [
        "opaque_\u{00ad}_alias",
        "opaque_\u{200b}_alias",
        "opaque_\u{2060}_alias",
        "opaque_\u{fe0f}_alias",
        "opaque_\u{e0001}_alias",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_header(
            &mut client,
            invalid_ref,
            &format!("session_snapshot_default_ignorable_{index}"),
        )
        .expect_err("snapshot references must reject default-ignorable aliases");
        assert_check(&error, "response_snapshot_snapshot_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_header(
            &mut client,
            &format!("snapshot_session_default_ignorable_{index}"),
            invalid_ref,
        )
        .expect_err("session references must reject default-ignorable aliases");
        assert_check(&error, "response_snapshot_session_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let snapshot_ref = format!("snapshot_event_default_ignorable_{index}");
        insert_header(
            &mut client,
            &snapshot_ref,
            &format!("session_event_default_ignorable_{index}"),
        )
        .unwrap();
        let error = insert_entry(
            &mut client,
            &snapshot_ref,
            invalid_ref,
            &format!("item_event_default_ignorable_{index}"),
        )
        .expect_err("event references must reject default-ignorable aliases");
        assert_check(&error, "response_snapshot_entry_event_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let snapshot_ref = format!("snapshot_item_default_ignorable_{index}");
        insert_header(
            &mut client,
            &snapshot_ref,
            &format!("session_item_default_ignorable_{index}"),
        )
        .unwrap();
        let error = insert_entry(
            &mut client,
            &snapshot_ref,
            &format!("event_item_default_ignorable_{index}"),
            invalid_ref,
        )
        .expect_err("item-version references must reject default-ignorable aliases");
        assert_check(
            &error,
            "response_snapshot_entry_item_version_ref_format_check",
        );
    }
}
