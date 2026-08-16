//! Real `PostgreSQL` bounds for immutable research-release approval rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_research_release::apply_research_release_migration;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    let schema_name = format!("research_release_schema_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_research_release_migration(&mut client).unwrap();
    (client, schema_name)
}

fn constraint_name(error: &postgres::Error) -> String {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
        .to_owned()
}

const VALID_COLUMNS: &str = "research_release_ref, dataset_snapshot_ref, research_scope_ref, \
     manifest_digest, privacy_review_ref, scientific_review_ref, metadata_bundle_ref, \
     license_record_ref, measurement_provenance_ref, access_approval_ref, \
     citation_metadata_ref, release_approver_ref, ordinary_admin_ref, access_class";

const VALID_VALUES: &str = "'research_release_schema_alpha', 'dataset_snapshot_schema_alpha', \
     'research_scope_schema_alpha', \
     'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
     'privacy_review_schema_alpha', 'scientific_review_schema_alpha', \
     'metadata_bundle_schema_alpha', 'license_record_schema_alpha', \
     'measurement_provenance_schema_alpha', 'access_approval_schema_alpha', \
     'citation_metadata_schema_alpha', 'release_approver_schema_alpha', \
     'ordinary_admin_schema_alpha', 'controlled'";

#[test]
fn schema_rejects_numeric_identity_invalid_digest_unknown_class_and_shared_approver() {
    let (mut client, schema_name) = isolated_client();

    let numeric = client
        .execute(
            &format!(
                "INSERT INTO research_release_approval ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace("'research_release_schema_alpha'", "'12'")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric),
        "research_release_approval_release_ref_format_check"
    );

    let whitespace = client
        .execute(
            &format!(
                "INSERT INTO research_release_approval ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace(
                    "'research_release_schema_alpha'",
                    "' research_release_schema_alpha '"
                )
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&whitespace),
        "research_release_approval_release_ref_format_check"
    );

    let digest = client
        .execute(
            &format!(
                "INSERT INTO research_release_approval ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "not-a-digest"
                )
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&digest),
        "research_release_approval_manifest_digest_format_check"
    );

    let access_class = client
        .execute(
            &format!(
                "INSERT INTO research_release_approval ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace("'controlled'", "'restricted'")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&access_class),
        "research_release_approval_access_class_value_check"
    );

    let duties = client
        .execute(
            &format!(
                "INSERT INTO research_release_approval ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace(
                    "'ordinary_admin_schema_alpha'",
                    "'release_approver_schema_alpha'"
                )
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&duties),
        "research_release_approval_separation_of_duties_check"
    );

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}
