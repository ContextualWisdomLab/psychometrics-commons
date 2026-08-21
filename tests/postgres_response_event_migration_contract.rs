//! `PostgreSQL` migration contracts for durable response-event history.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::apply_response_event_migration;

fn test_client(schema_name: &str) -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema_name}; SET search_path TO {schema_name}; DROP TABLE IF EXISTS response_event;"
        ))
        .unwrap();
    client
}

#[test]
fn migration_rejects_an_incompatible_preexisting_response_event_schema() {
    let mut client = test_client("response_event_migration_shape_test");
    client
        .batch_execute(
            "CREATE TABLE response_event (response_event_ref TEXT PRIMARY KEY, session_ref TEXT);",
        )
        .unwrap();

    let error = apply_response_event_migration(&mut client)
        .expect_err("migration must fail closed instead of accepting a partial preexisting table");
    let database_error = error
        .as_db_error()
        .expect("schema-contract rejection must be a PostgreSQL database error");
    assert_eq!(database_error.code().code(), "55000");
    assert!(database_error.message().contains("response_event"));
    assert!(database_error.message().contains("contract"));
}

#[test]
fn migration_rejects_unicode_numeric_like_reference_aliases() {
    let mut client = test_client("response_event_reference_constraint_test");
    apply_response_event_migration(&mut client).unwrap();

    let columns = [
        (
            "response_event_ref",
            "response_event_response_event_ref_format_check",
        ),
        ("session_ref", "response_event_session_ref_format_check"),
        (
            "client_event_ref",
            "response_event_client_event_ref_format_check",
        ),
        (
            "item_version_ref",
            "response_event_item_version_ref_format_check",
        ),
    ];
    let numeric_like_aliases = ["12．34", "12٫34", "12٬34", "12，34"];
    let mut case_index = 0_i64;

    for (column_name, expected_constraint) in columns {
        for alias in numeric_like_aliases {
            case_index += 1;
            let mut response_event_ref = format!("response_event_case_{case_index}");
            let mut session_ref = format!("session_case_{case_index}");
            let mut client_event_ref = format!("client_event_case_{case_index}");
            let mut item_version_ref = format!("item_version_case_{case_index}");
            match column_name {
                "response_event_ref" => response_event_ref = alias.to_owned(),
                "session_ref" => session_ref = alias.to_owned(),
                "client_event_ref" => client_event_ref = alias.to_owned(),
                "item_version_ref" => item_version_ref = alias.to_owned(),
                _ => unreachable!("test enumerates only response-event reference columns"),
            }

            let error = client
                .execute(
                    "INSERT INTO response_event (\
                         response_event_ref, session_ref, client_event_ref, item_version_ref, \
                         payload_digest, server_sequence, observed_at, received_at\
                     ) VALUES ($1, $2, $3, $4, $5, $6, \
                               TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                               TIMESTAMPTZ '2023-11-14 22:13:20.250+00')",
                    &[
                        &response_event_ref,
                        &session_ref,
                        &client_event_ref,
                        &item_version_ref,
                        &"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        &case_index,
                    ],
                )
                .expect_err(
                    "Unicode numeric-like aliases must fail the owned reference constraint",
                );
            assert_eq!(
                error
                    .as_db_error()
                    .and_then(postgres::error::DbError::constraint),
                Some(expected_constraint),
                "column {column_name} accepted or misclassified alias {alias:?}"
            );
        }
    }
}
