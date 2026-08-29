//! Direct PostgreSQL writes must preserve the Rust opaque-reference boundary for participants.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_participant::apply_participant_base_migration;

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS participant_reference_parity_test CASCADE;\
             CREATE SCHEMA participant_reference_parity_test;\
             SET search_path TO participant_reference_parity_test;",
        )
        .unwrap();
    apply_participant_base_migration(&mut client).unwrap();
    client
}

fn assert_reference_rejected(client: &mut Client, participant_ref: &str, tenant_ref: &str) {
    let error = client
        .execute(
            "INSERT INTO assessment_participant\
                 (participant_ref, tenant_ref, created_at_unix_ms)\
             VALUES ($1, $2, 40000)",
            &[&participant_ref, &tenant_ref],
        )
        .expect_err("a Rust-invalid participant reference must fail at the database boundary");
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
}

#[test]
fn participant_and_tenant_references_reject_control_and_default_ignorable_aliases() {
    let mut client = client();
    let invalid_references = [
        "opaque_\u{0001}_alpha",
        "opaque_\u{00ad}_alpha",
        "opaque_\u{200b}_alpha",
        "opaque_\u{200d}_alpha",
        "opaque_\u{2060}_alpha",
        "opaque_\u{fe0f}_alpha",
        "opaque_\u{feff}_alpha",
        "opaque_\u{e0001}_alpha",
    ];

    for invalid_ref in invalid_references {
        assert_reference_rejected(&mut client, invalid_ref, "tenant_valid_alpha");
        assert_reference_rejected(&mut client, "participant_valid_alpha", invalid_ref);
    }

    let stored_rows: i64 = client
        .query_one("SELECT count(*) FROM assessment_participant", &[])
        .unwrap()
        .get(0);
    assert_eq!(stored_rows, 0);
}

#[test]
fn migration_reapplication_fails_closed_on_historical_default_ignorable_identity() {
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE assessment_participant\
                 DROP CONSTRAINT assessment_participant_ref_format_check;\
             ALTER TABLE assessment_participant\
                 ADD CONSTRAINT assessment_participant_ref_format_check\
                 CHECK (participant_ref <> '');",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO assessment_participant\
                 (participant_ref, tenant_ref, created_at_unix_ms)\
             VALUES ($1, 'tenant_historical_alpha', 40000)",
            &[&"participant_\u{200b}_historical"],
        )
        .expect("the deliberately weakened historical constraint must admit the regression row");

    let error = apply_participant_base_migration(&mut client).expect_err(
        "migration reapplication must reject historical identity Rust cannot reconstruct",
    );
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));

    let preserved: i64 = client
        .query_one(
            "SELECT count(*) FROM assessment_participant WHERE participant_ref = $1",
            &[&"participant_\u{200b}_historical"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        preserved, 1,
        "migration must not silently rewrite immutable identity"
    );
}
