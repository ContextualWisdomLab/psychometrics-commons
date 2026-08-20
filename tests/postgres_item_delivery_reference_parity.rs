//! Item-delivery persistence must match the Rust opaque-reference boundary.
//!
//! The domain trims Unicode whitespace and rejects embedded control characters and numeric-like
//! spellings under Rust `char::is_numeric`. Direct SQL, array evidence, and migration upgrades
//! must not preserve aliases the domain would reject or normalize.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
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
            "DROP SCHEMA IF EXISTS item_delivery_reference_parity_test CASCADE; \
             CREATE SCHEMA item_delivery_reference_parity_test; \
             SET search_path TO item_delivery_reference_parity_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_ledger(
    client: &mut Client,
    tenant_ref: &str,
    session_ref: &str,
    release_ref: &str,
    allowed_items: &[&str],
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO item_delivery_ledger (\
             tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
             allowed_item_version_refs\
         ) VALUES ($1,$2,$3,$4,'en-US',$5)",
        &[
            &tenant_ref,
            &session_ref,
            &release_ref,
            &DIGEST,
            &allowed_items,
        ],
    )
}

fn insert_event(
    client: &mut Client,
    tenant_ref: &str,
    session_ref: &str,
    delivery_ref: &str,
    item_ref: &str,
    presentation_ref: &str,
    selection_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO item_delivery_event (\
             tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
             presentation_context_ref, selection_evidence_ref, delivery_sequence\
         ) VALUES ($1,$2,$3,$4,$5,$6,1)",
        &[
            &tenant_ref,
            &session_ref,
            &delivery_ref,
            &item_ref,
            &presentation_ref,
            &selection_ref,
        ],
    )
}

#[test]
fn ledger_scalar_and_array_references_reject_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "12345",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_ledger(
            &mut client,
            invalid_ref,
            &format!("session_tenant_{index}"),
            &format!("release_tenant_{index}"),
            &["item_alpha"],
        )
        .expect_err("tenant references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_ledger_tenant_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_ledger(
            &mut client,
            "tenant_alpha",
            invalid_ref,
            &format!("release_session_{index}"),
            &["item_alpha"],
        )
        .expect_err("session references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_ledger_session_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_ledger(
            &mut client,
            "tenant_alpha",
            &format!("session_release_{index}"),
            invalid_ref,
            &["item_alpha"],
        )
        .expect_err("release references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_ledger_release_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_ledger(
            &mut client,
            "tenant_alpha",
            &format!("session_array_{index}"),
            &format!("release_array_{index}"),
            &["item_alpha", invalid_ref],
        )
        .expect_err("allowed-item arrays must enforce every Rust reference predicate");
        assert_check(&error, "item_delivery_ledger_allowed_items_format_check");
    }

    for (index, allowed_items) in [
        vec!["item_2", "opaque_alpha 2"],
        vec!["release_3.1", "v1-2"],
    ]
    .into_iter()
    .enumerate()
    {
        insert_ledger(
            &mut client,
            "tenant_mixed",
            &format!("session_mixed_{index}"),
            &format!("release_mixed_{index}"),
            &allowed_items,
        )
        .expect("mixed references must remain valid opaque identifiers");
    }

    let null_array: bool = client
        .query_one(
            "SELECT item_delivery_reference_array_is_valid(NULL::text[])",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(!null_array, "a NULL reference array must fail closed");
}

#[test]
fn delivery_event_references_reject_rust_invalid_aliases() {
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
        let session_ref = format!("session_delivery_{index}");
        let item_ref = format!("item_delivery_{index}");
        insert_ledger(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("release_delivery_{index}"),
            &[item_ref.as_str()],
        )
        .unwrap();
        let error = insert_event(
            &mut client,
            "tenant_alpha",
            &session_ref,
            invalid_ref,
            &item_ref,
            &format!("presentation_delivery_{index}"),
            None,
        )
        .expect_err("delivery references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_event_delivery_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let session_ref = format!("session_item_{index}");
        insert_ledger(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("release_item_{index}"),
            &[invalid_ref],
        )
        .expect_err("invalid item identity must already fail in allowed-item evidence");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let session_ref = format!("session_presentation_{index}");
        let item_ref = format!("item_presentation_{index}");
        insert_ledger(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("release_presentation_{index}"),
            &[item_ref.as_str()],
        )
        .unwrap();
        let error = insert_event(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("delivery_presentation_{index}"),
            &item_ref,
            invalid_ref,
            None,
        )
        .expect_err("presentation references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_event_presentation_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let session_ref = format!("session_selection_{index}");
        let item_ref = format!("item_selection_{index}");
        insert_ledger(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("release_selection_{index}"),
            &[item_ref.as_str()],
        )
        .unwrap();
        let error = insert_event(
            &mut client,
            "tenant_alpha",
            &session_ref,
            &format!("delivery_selection_{index}"),
            &item_ref,
            &format!("presentation_selection_{index}"),
            Some(invalid_ref),
        )
        .expect_err("selection references must match the Rust opaque-reference boundary");
        assert_check(&error, "item_delivery_event_selection_ref_format_check");
    }
}

#[test]
fn migration_reapplication_revalidates_existing_rows_under_the_rust_predicate() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION item_delivery_reference_is_valid(reference_value TEXT) \
             RETURNS BOOLEAN LANGUAGE SQL IMMUTABLE PARALLEL SAFE SET search_path = pg_catalog AS $$ \
                 SELECT reference_value IS NOT NULL AND reference_value <> '' \
             $$;",
        )
        .unwrap();
    insert_ledger(
        &mut client,
        "½",
        "session_upgrade_guard",
        "release_upgrade_guard",
        &["item_upgrade_guard"],
    )
    .expect("the deliberately weakened historical predicate should admit the regression row");

    let error = apply_item_delivery_migration(&mut client).expect_err(
        "migration reapplication must revalidate pre-existing rows under the new predicate",
    );
    assert_check(&error, "item_delivery_ledger_tenant_ref_format_check");
}
