//! Database-boundary regression tests for canonical item-delivery references.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

const SCHEMA: &str = "item_delivery_reference_whitespace_test";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn constraint_name(error: &postgres::Error) -> &str {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
}

#[test]
fn database_rejects_outer_control_whitespace_in_scalar_and_array_references() {
    let mut client = test_client();
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;\
             CREATE SCHEMA {SCHEMA};\
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated reference-whitespace schema should be reset");
    apply_item_delivery_migration(&mut client)
        .expect("item-delivery migration should install the physical schema");

    for (invalid_session_ref, expected_constraint) in [
        (
            "\tsession_reference_tab",
            "item_delivery_ledger_session_ref_format_check",
        ),
        (
            "session_reference_newline\n",
            "item_delivery_ledger_session_ref_format_check",
        ),
    ] {
        let error = client
            .execute(
                "INSERT INTO item_delivery_ledger (\
                     tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                     release_content_digest, locale, allowed_item_version_refs\
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &"tenant_reference_alpha",
                    &invalid_session_ref,
                    &"release_reference_alpha",
                    &"instrument_version_ko_v1",
                    &"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    &"en-US",
                    &vec!["item_reference_alpha"],
                ],
            )
            .expect_err("outer control whitespace must fail at the database boundary");
        assert_eq!(constraint_name(&error), expected_constraint);
    }

    for invalid_item_ref in ["\titem_reference_tab", "item_reference_newline\n"] {
        let session_ref = format!(
            "session_{}",
            invalid_item_ref.replace(['\t', '\n'], "control")
        );
        let error = client
            .execute(
                "INSERT INTO item_delivery_ledger (\
                     tenant_ref, session_ref, instrument_release_ref, instrument_version_ref, \
                     release_content_digest, locale, allowed_item_version_refs\
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &"tenant_reference_alpha",
                    &session_ref,
                    &"release_reference_alpha",
                    &"instrument_version_ko_v1",
                    &"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    &"en-US",
                    &vec![invalid_item_ref],
                ],
            )
            .expect_err("outer control whitespace in allowed-item evidence must fail closed");
        assert_eq!(
            constraint_name(&error),
            "item_delivery_ledger_allowed_items_format_check"
        );
    }

    client
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"))
        .expect("isolated reference-whitespace schema should be removed");
}
