//! Real `PostgreSQL` parity contracts for durable response-event references.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::apply_response_event_migration;
use psychometrics_commons_runtime::response::ResponseEvent;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS response_event_reference_parity_test;\
             SET search_path TO response_event_reference_parity_test;\
             DROP TABLE IF EXISTS response_event;\
             DROP FUNCTION IF EXISTS response_event_reference_is_valid(TEXT);",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
}

fn insert_reference_tuple(
    client: &mut Client,
    response_event_ref: &str,
    session_ref: &str,
    client_event_ref: &str,
    item_version_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO response_event (\
             response_event_ref, session_ref, client_event_ref, item_version_ref, \
             payload_digest, server_sequence, observed_at, received_at\
         ) VALUES ($1, $2, $3, $4, \
                   'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
                   1, to_timestamp(1700000000), to_timestamp(1700000001))",
        &[
            &response_event_ref,
            &session_ref,
            &client_event_ref,
            &item_version_ref,
        ],
    )
}

#[test]
fn direct_sql_rejects_every_rust_invalid_response_reference_family() {
    let _guard = test_guard();
    let mut client = test_client();
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}response_event_alias\u{00a0}",
        "response\u{0007}event",
        "response\u{00ad}event",
        "response\u{200b}event",
        "response\u{200d}event",
        "response\u{2060}event",
        "response\u{fe0f}event",
        "response\u{feff}event",
        "response\u{e0001}event",
    ];

    for invalid_reference in invalid_references {
        for invalid_column in 0..4 {
            let mut references = [
                "response_event_valid",
                "session_valid",
                "client_event_valid",
                "item_version_valid",
            ];
            references[invalid_column] = invalid_reference;
            let error = insert_reference_tuple(
                &mut client,
                references[0],
                references[1],
                references[2],
                references[3],
            )
            .expect_err("a Rust-invalid durable reference must fail at the database boundary");
            assert_eq!(
                error.as_db_error().map(postgres::error::DbError::code),
                Some(&postgres::error::SqlState::CHECK_VIOLATION)
            );
        }
    }

    let persisted_rows: i64 = client
        .query_one("SELECT COUNT(*) FROM response_event", &[])
        .unwrap()
        .get(0);
    assert_eq!(persisted_rows, 0);
}

#[test]
fn sql_numeric_validation_matches_rust_for_every_unicode_scalar() {
    let _guard = test_guard();
    let mut client = test_client();
    let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let rust_numeric: Vec<i32> = (1..=0x0010_ffff)
        .filter_map(|codepoint| {
            let character = char::from_u32(codepoint)?;
            if !character.is_numeric() {
                return None;
            }
            let reference = character.to_string();
            assert!(ResponseEvent::from_persisted(
                reference,
                "client_event_valid",
                "item_version_valid",
                digest,
                1,
            )
            .is_err());
            Some(i32::try_from(codepoint).expect("Unicode scalar values fit in PostgreSQL int4"))
        })
        .collect();
    let sql_invalid: Vec<i32> = client
        .query_one(
            r"SELECT ARRAY(
                SELECT codepoint
                FROM unnest($1::int4[]) AS codepoint
                WHERE NOT response_event_reference_is_valid(chr(codepoint))
                ORDER BY codepoint)",
            &[&rust_numeric],
        )
        .unwrap()
        .get(0);

    assert_eq!(rust_numeric, sql_invalid);
    assert!(ResponseEvent::from_persisted(
        "\0",
        "client_event_valid",
        "item_version_valid",
        digest,
        1,
    )
    .is_err());
}

#[test]
fn migration_reapplication_repairs_a_weakened_owned_reference_constraint() {
    let _guard = test_guard();
    let mut client = test_client();
    client
        .batch_execute(
            "ALTER TABLE response_event \
                 DROP CONSTRAINT response_event_response_event_ref_format_check;\
             ALTER TABLE response_event \
                 ADD CONSTRAINT response_event_response_event_ref_format_check \
                 CHECK (response_event_ref <> '');",
        )
        .unwrap();

    apply_response_event_migration(&mut client)
        .expect("reapplication must replace and revalidate the migration-owned reference check");

    let error = insert_reference_tuple(
        &mut client,
        "½",
        "session_valid",
        "client_event_valid",
        "item_version_valid",
    )
    .expect_err("the repaired constraint must reject Rust-invalid identity");
    assert_eq!(
        error.as_db_error().map(postgres::error::DbError::code),
        Some(&postgres::error::SqlState::CHECK_VIOLATION)
    );
}

#[test]
fn migration_reapplication_fails_closed_when_historical_invalid_identity_exists() {
    let _guard = test_guard();
    let mut client = test_client();
    client
        .batch_execute(
            "ALTER TABLE response_event \
                 DROP CONSTRAINT response_event_response_event_ref_format_check;\
             ALTER TABLE response_event \
                 ADD CONSTRAINT response_event_response_event_ref_format_check \
                 CHECK (response_event_ref <> '');",
        )
        .unwrap();
    insert_reference_tuple(
        &mut client,
        "½",
        "session_valid",
        "client_event_valid",
        "item_version_valid",
    )
    .expect("the deliberately weakened historical constraint must admit the fixture");

    let error = apply_response_event_migration(&mut client)
        .expect_err("upgrade must not bless a historical identity Rust cannot reconstruct");
    assert_eq!(
        error.as_db_error().map(postgres::error::DbError::code),
        Some(&postgres::error::SqlState::CHECK_VIOLATION)
    );
}
