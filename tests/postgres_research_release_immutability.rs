//! Database-level immutability coverage for approved Research Commons release evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_research_release::apply_research_release_migration;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_schema_name() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    format!(
        "research_release_immutability_{}_{}",
        std::process::id(),
        nonce
    )
}

fn assert_immutable_guard(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("immutable release mutations must fail at the database boundary");
    assert_eq!(database_error.code().code(), "55000");
}

#[test]
fn approved_research_release_rows_reject_update_delete_and_truncate() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let schema_name = isolated_schema_name();
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_research_release_migration(&mut client).unwrap();

    client
        .execute(
            "INSERT INTO research_release_approval (\
                 research_release_ref, dataset_snapshot_ref, research_scope_ref, manifest_digest,\
                 privacy_review_ref, scientific_review_ref, metadata_bundle_ref, license_record_ref,\
                 measurement_provenance_ref, access_approval_ref, citation_metadata_ref,\
                 release_approver_ref, ordinary_admin_ref, access_class\
             ) VALUES (\
                 'research_release_immutable', 'dataset_snapshot_immutable', 'research_scope_immutable',\
                 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                 'privacy_review_immutable', 'scientific_review_immutable', 'metadata_bundle_immutable',\
                 'license_record_immutable', 'measurement_provenance_immutable',\
                 'access_approval_immutable', 'citation_metadata_immutable',\
                 'release_approver_immutable', 'ordinary_admin_immutable', 'controlled'\
             )",
            &[],
        )
        .unwrap();

    let update_error = client
        .execute(
            "UPDATE research_release_approval \
             SET scientific_review_ref = 'scientific_review_tampered' \
             WHERE research_release_ref = 'research_release_immutable'",
            &[],
        )
        .expect_err("approved release evidence must reject in-place update");
    assert_immutable_guard(&update_error);

    let delete_error = client
        .execute(
            "DELETE FROM research_release_approval \
             WHERE research_release_ref = 'research_release_immutable'",
            &[],
        )
        .expect_err("approved release evidence must reject in-place deletion");
    assert_immutable_guard(&delete_error);

    let truncate_error = client
        .batch_execute("TRUNCATE TABLE research_release_approval;")
        .expect_err("approved release evidence must reject table truncation");
    assert_immutable_guard(&truncate_error);

    let row = client
        .query_one(
            "SELECT scientific_review_ref, access_class \
             FROM research_release_approval \
             WHERE research_release_ref = 'research_release_immutable'",
            &[],
        )
        .unwrap();
    assert_eq!(
        row.get::<_, String>(0),
        "scientific_review_immutable",
        "failed mutation must preserve original review evidence"
    );
    assert_eq!(row.get::<_, String>(1), "controlled");

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}
