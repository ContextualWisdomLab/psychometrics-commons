//! Real `PostgreSQL` contracts for durable item-delivery reference canonicality.
//!
//! The product domain rejects Unicode default-ignorable aliases and numeric-only identity
//! spellings. Durable SQL/restore/operator paths must enforce the same acceptance grammar. A
//! reference-policy upgrade must revalidate rows written under weaker historical semantics, while
//! an ordinary idempotent migration reapply must preserve already-current constraint objects.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ITEM_DELIVERY_REFERENCE_FIXTURE_LOCK_KEY: i64 = 0x4954_4452_4341_4E4F;
const REFERENCE_CONSTRAINTS: [&str; 9] = [
    "item_delivery_event_delivery_ref_format_check",
    "item_delivery_event_item_ref_format_check",
    "item_delivery_event_presentation_ref_format_check",
    "item_delivery_event_selection_ref_format_check",
    "item_delivery_event_tenant_ref_format_check",
    "item_delivery_ledger_allowed_items_format_check",
    "item_delivery_ledger_release_ref_format_check",
    "item_delivery_ledger_session_ref_format_check",
    "item_delivery_ledger_tenant_ref_format_check",
];

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .query_one(
            "SELECT set_config('lock_timeout', '60s', false), pg_advisory_lock($1)",
            &[&ITEM_DELIVERY_REFERENCE_FIXTURE_LOCK_KEY],
        )
        .expect(
            "shared item-delivery reference fixture lock must be acquired within sixty seconds",
        );
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_reference_canonicality_test CASCADE; \
             CREATE SCHEMA item_delivery_reference_canonicality_test; \
             SET search_path TO item_delivery_reference_canonicality_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must be a PostgreSQL constraint failure");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn reference_constraint_oids(client: &mut Client) -> Vec<(String, String)> {
    let constraint_names: Vec<String> = REFERENCE_CONSTRAINTS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let rows = client
        .query(
            "SELECT conname, oid::text \
             FROM pg_constraint \
             WHERE conname = ANY($1::text[]) \
               AND connamespace = current_schema()::regnamespace \
               AND conrelid IN ('item_delivery_ledger'::regclass, 'item_delivery_event'::regclass) \
             ORDER BY conname",
            &[&constraint_names],
        )
        .expect("reference constraints must be inspectable");
    assert_eq!(
        rows.len(),
        REFERENCE_CONSTRAINTS.len(),
        "all predicate-dependent item-delivery reference constraints must exist"
    );
    rows.into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

#[test]
fn scalar_and_array_predicates_reject_rust_invalid_aliases() {
    let mut client = client();

    for reference in [
        "tenant\u{200b}_alpha",
        "tenant\u{feff}_alpha",
        "tenant\u{e0001}_alpha",
        "\u{00a0}tenant_alpha",
        "tenant_alpha\u{00a0}",
        "tenant\u{0085}_alpha",
        "½",
        "Ⅳ",
    ] {
        let accepted: bool = client
            .query_one("SELECT item_delivery_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            !accepted,
            "durable scalar predicate accepted Rust-invalid alias: {reference:?}"
        );
    }

    let invalid_array = vec!["item_alpha", "item\u{200d}_alpha"];
    let accepted: bool = client
        .query_one(
            "SELECT item_delivery_reference_array_is_valid($1)",
            &[&invalid_array],
        )
        .unwrap()
        .get(0);
    assert!(
        !accepted,
        "durable array predicate must reuse scalar canonicality"
    );

    for reference in ["tenant_2", "tenant_가나다_東京", "opaque alpha 2", "v1-2"] {
        let accepted: bool = client
            .query_one("SELECT item_delivery_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            accepted,
            "visible mixed opaque reference must remain valid: {reference:?}"
        );
    }
}

#[test]
fn current_policy_reapply_preserves_reference_constraint_objects() {
    let mut client = client();
    let before = reference_constraint_oids(&mut client);

    apply_item_delivery_migration(&mut client)
        .expect("an already-current item-delivery migration must remain idempotent");

    let after = reference_constraint_oids(&mut client);
    assert_eq!(
        after, before,
        "an unchanged reference policy must not drop and rebuild validated constraints"
    );
}

#[test]
fn migration_reapply_revalidates_rows_written_under_a_weaker_predicate() {
    let mut client = client();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION item_delivery_reference_is_valid(reference_value TEXT) \
             RETURNS BOOLEAN LANGUAGE SQL IMMUTABLE PARALLEL SAFE SET search_path = pg_catalog AS $$ \
                 SELECT reference_value IS NOT NULL AND reference_value <> '' \
             $$; \
             COMMENT ON FUNCTION item_delivery_reference_is_valid(TEXT) IS NULL;",
        )
        .unwrap();

    let unsafe_tenant = "tenant\u{200b}_historical";
    client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
                 allowed_item_version_refs\
             ) VALUES ($1, 'session_reference_upgrade', 'release_reference_upgrade', $2, 'en-US', \
                       ARRAY['item_reference_upgrade'])",
            &[&unsafe_tenant, &DIGEST],
        )
        .expect("the deliberately weakened historical predicate should admit the regression row");

    let error = apply_item_delivery_migration(&mut client)
        .expect_err("reference-policy upgrade must scan and reject the historical invisible alias");
    assert_check(&error, "item_delivery_ledger_tenant_ref_format_check");
}

#[test]
fn migration_reapply_revalidates_rows_after_array_predicate_changes() {
    let mut client = client();

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION item_delivery_reference_array_is_valid(reference_values TEXT[]) \
             RETURNS BOOLEAN LANGUAGE SQL IMMUTABLE PARALLEL SAFE \
             SET search_path = pg_catalog, item_delivery_reference_canonicality_test AS $$ \
                 SELECT reference_values IS NOT NULL \
             $$;",
        )
        .unwrap();

    client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
                 allowed_item_version_refs\
             ) VALUES ('tenant_array_upgrade', 'session_array_upgrade', 'release_array_upgrade', \
                       $1, 'en-US', ARRAY['item_duplicate', 'item_duplicate'])",
            &[&DIGEST],
        )
        .expect("the deliberately weakened array predicate should admit duplicate item identities");

    let error = apply_item_delivery_migration(&mut client)
        .expect_err("array-policy upgrade must rescan and reject the historical duplicate item set");
    assert_check(&error, "item_delivery_ledger_allowed_items_format_check");
}
