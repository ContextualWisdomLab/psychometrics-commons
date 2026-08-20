//! Complete direct-SQL coverage for item-delivery event reference constraints.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_item_delivery::apply_item_delivery_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str =
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
            "DROP SCHEMA IF EXISTS item_delivery_event_reference_parity_test CASCADE; \
             CREATE SCHEMA item_delivery_event_reference_parity_test; \
             SET search_path TO item_delivery_event_reference_parity_test;",
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

fn seed_ledger(client: &mut Client, suffix: &str) -> String {
    let session_ref = format!("session_event_parity_{suffix}");
    let item_ref = format!("item_event_parity_{suffix}");
    client
        .execute(
            "INSERT INTO item_delivery_ledger (\
                 tenant_ref, session_ref, instrument_release_ref, release_content_digest, locale, \
                 allowed_item_version_refs\
             ) VALUES ('tenant_event_parity',$1,$2,$3,'en-US',$4)",
            &[
                &session_ref,
                &format!("release_event_parity_{suffix}"),
                &DIGEST,
                &vec![item_ref.as_str()],
            ],
        )
        .unwrap();
    session_ref
}

#[test]
fn event_tenant_and_item_columns_apply_the_same_rust_reference_boundary() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let session_ref = seed_ledger(&mut client, &format!("tenant_{index}"));
        let error = client
            .execute(
                "INSERT INTO item_delivery_event (\
                     tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                     presentation_context_ref, delivery_sequence\
                 ) VALUES ($1,$2,$3,$4,$5,1)",
                &[
                    &invalid_ref,
                    &session_ref,
                    &format!("delivery_tenant_{index}"),
                    &format!("item_tenant_{index}"),
                    &format!("presentation_tenant_{index}"),
                ],
            )
            .expect_err("event tenant aliases must fail before foreign-key classification");
        assert_check(&error, "item_delivery_event_tenant_ref_format_check");
    }

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let session_ref = seed_ledger(&mut client, &format!("item_{index}"));
        let error = client
            .execute(
                "INSERT INTO item_delivery_event (\
                     tenant_ref, session_ref, delivery_event_ref, item_version_ref, \
                     presentation_context_ref, delivery_sequence\
                 ) VALUES ('tenant_event_parity',$1,$2,$3,$4,1)",
                &[
                    &session_ref,
                    &format!("delivery_item_{index}"),
                    &invalid_ref,
                    &format!("presentation_item_{index}"),
                ],
            )
            .expect_err("event item-version aliases must fail closed at their named CHECK");
        assert_check(&error, "item_delivery_event_item_ref_format_check");
    }
}
