//! Data-rights persistence must match the Rust opaque-reference boundary.
//!
//! Privacy/export/deletion evidence cannot safely retain an identifier that the application would
//! reject or normalize differently. These tests exercise request, propagation, and identity-
//! verification columns through direct SQL, including Unicode numeric aliases, Unicode outer
//! whitespace, embedded controls, and upgrade revalidation.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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
            "DROP SCHEMA IF EXISTS data_rights_reference_parity_test CASCADE; \
             CREATE SCHEMA data_rights_reference_parity_test; \
             SET search_path TO data_rights_reference_parity_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

#[allow(clippy::too_many_arguments)]
fn insert_request(
    client: &mut Client,
    request_ref: &str,
    tenant_ref: &str,
    participant_ref: &str,
    scope_ref: &str,
    verification_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    let verified_at = verification_ref.map(|_| 11_000_i64);
    client.execute(
        "INSERT INTO data_rights_request_state (\
             request_ref, tenant_ref, participant_ref, request_kind, scope_ref, current_state, \
             requested_at_unix_ms, latest_event_at_unix_ms, verification_evidence_ref, \
             verified_at_unix_ms\
         ) VALUES ($1,$2,$3,'deletion',$4,'requested',10000,10000,$5,$6)",
        &[
            &request_ref,
            &tenant_ref,
            &participant_ref,
            &scope_ref,
            &verification_ref,
            &verified_at,
        ],
    )
}

fn seed_request_and_outbox(client: &mut Client, suffix: &str) -> (String, String, String) {
    let request_ref = format!("data_rights_request_{suffix}");
    let tenant_ref = format!("tenant_{suffix}");
    let event_ref = format!("data_rights_event_{suffix}");
    insert_request(
        client,
        &request_ref,
        &tenant_ref,
        &format!("participant_{suffix}"),
        &format!("scope_{suffix}"),
        None,
    )
    .unwrap();
    client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref, \
                 occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts, \
                 latest_event_at_unix_ms\
             ) VALUES ($1,'data_rights.deletion.requested','v1','psychometrics_commons',$2,$3,\
                       10000,$3,$4,3,10000)",
            &[&event_ref, &tenant_ref, &request_ref, &DIGEST],
        )
        .unwrap();
    (request_ref, tenant_ref, event_ref)
}

#[test]
fn request_and_verification_references_reject_rust_invalid_aliases() {
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
        let error = insert_request(
            &mut client,
            invalid_ref,
            &format!("tenant_request_{index}"),
            &format!("participant_request_{index}"),
            &format!("scope_request_{index}"),
            None,
        )
        .expect_err("request references must match the Rust opaque-reference boundary");
        assert_check(&error, "data_rights_request_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_request(
            &mut client,
            &format!("data_rights_tenant_{index}"),
            invalid_ref,
            &format!("participant_tenant_{index}"),
            &format!("scope_tenant_{index}"),
            None,
        )
        .expect_err("tenant references must match the Rust opaque-reference boundary");
        assert_check(&error, "data_rights_tenant_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_request(
            &mut client,
            &format!("data_rights_participant_{index}"),
            &format!("tenant_participant_{index}"),
            invalid_ref,
            &format!("scope_participant_{index}"),
            None,
        )
        .expect_err("participant references must match the Rust opaque-reference boundary");
        assert_check(&error, "data_rights_participant_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_request(
            &mut client,
            &format!("data_rights_scope_{index}"),
            &format!("tenant_scope_{index}"),
            &format!("participant_scope_{index}"),
            invalid_ref,
            None,
        )
        .expect_err("scope references must match the Rust opaque-reference boundary");
        assert_check(&error, "data_rights_scope_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_request(
            &mut client,
            &format!("data_rights_verification_{index}"),
            &format!("tenant_verification_{index}"),
            &format!("participant_verification_{index}"),
            &format!("scope_verification_{index}"),
            Some(invalid_ref),
        )
        .expect_err("verification evidence must match the Rust opaque-reference boundary");
        assert_check(&error, "data_rights_verification_evidence_format_check");
    }

    assert_eq!(
        insert_request(
            &mut client,
            "request_item_2",
            "tenant_alpha 2",
            "participant_3.1",
            "scope-v1",
            Some("verification 2"),
        )
        .unwrap(),
        1
    );
}

#[test]
fn propagation_references_reject_rust_invalid_aliases() {
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
        let (request_ref, tenant_ref, event_ref) =
            seed_request_and_outbox(&mut client, &format!("dependent_{index}"));
        let error = client
            .execute(
                "INSERT INTO data_rights_propagation_state (\
                     request_ref, tenant_ref, dependent_system_ref, source_ref, event_ref, \
                     latest_event_at_unix_ms\
                 ) VALUES ($1,$2,$3,'psychometrics_commons',$4,10000)",
                &[&request_ref, &tenant_ref, &invalid_ref, &event_ref],
            )
            .expect_err("dependent-system aliases must fail closed at PostgreSQL");
        assert_check(&error, "data_rights_dependent_system_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let (request_ref, tenant_ref, event_ref) =
            seed_request_and_outbox(&mut client, &format!("source_{index}"));
        let error = client
            .execute(
                "INSERT INTO data_rights_propagation_state (\
                     request_ref, tenant_ref, dependent_system_ref, source_ref, event_ref, \
                     latest_event_at_unix_ms\
                 ) VALUES ($1,$2,$3,$4,$5,10000)",
                &[
                    &request_ref,
                    &tenant_ref,
                    &format!("dependent_source_{index}"),
                    &invalid_ref,
                    &event_ref,
                ],
            )
            .expect_err("source aliases must fail closed before outbox foreign-key classification");
        assert_check(&error, "data_rights_propagation_source_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let (request_ref, tenant_ref, _event_ref) =
            seed_request_and_outbox(&mut client, &format!("event_{index}"));
        let error = client
            .execute(
                "INSERT INTO data_rights_propagation_state (\
                     request_ref, tenant_ref, dependent_system_ref, source_ref, event_ref, \
                     latest_event_at_unix_ms\
                 ) VALUES ($1,$2,$3,'psychometrics_commons',$4,10000)",
                &[
                    &request_ref,
                    &tenant_ref,
                    &format!("dependent_event_{index}"),
                    &invalid_ref,
                ],
            )
            .expect_err("event aliases must fail closed before outbox foreign-key classification");
        assert_check(&error, "data_rights_propagation_event_ref_format_check");
    }
}

#[test]
fn migration_reapplication_revalidates_existing_request_rows() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE data_rights_request_state \
                 DROP CONSTRAINT data_rights_request_ref_format_check; \
             ALTER TABLE data_rights_request_state \
                 ADD CONSTRAINT data_rights_request_ref_format_check CHECK (\
                     request_ref = btrim(request_ref) AND request_ref <> ''\
                 );",
        )
        .unwrap();
    insert_request(
        &mut client,
        "½",
        "tenant_upgrade_guard",
        "participant_upgrade_guard",
        "scope_upgrade_guard",
        None,
    )
    .expect("the deliberately weakened historical constraint should admit the regression row");

    let error = apply_data_rights_migration(&mut client).expect_err(
        "migration reapplication must revalidate existing rows under the Rust predicate",
    );
    assert_check(&error, "data_rights_request_ref_format_check");
}
