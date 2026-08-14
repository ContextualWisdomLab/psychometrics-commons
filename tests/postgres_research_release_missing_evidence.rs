//! Regression coverage for a replay whose conflicting immutable evidence disappears before classification.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_research_release::{
    apply_research_release_migration, persist_approved_research_release,
    ResearchReleasePersistenceDisposition, ResearchReleasePersistenceError,
};
use psychometrics_commons_runtime::research_release::{
    approve_research_release, ResearchAccessClass, ResearchReleaseCandidate,
};
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn isolated_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    let schema_name = format!("research_release_missing_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_research_release_migration(&mut client).unwrap();
    (client, schema_name)
}

fn approved_release() -> psychometrics_commons_runtime::research_release::ApprovedResearchRelease {
    approve_research_release(ResearchReleaseCandidate {
        release_ref: "research_release_missing_replay",
        dataset_snapshot_ref: "dataset_snapshot_missing_replay",
        research_scope_ref: "research_scope_missing_replay",
        manifest_digest: DIGEST,
        privacy_review_ref: "privacy_review_missing_replay",
        scientific_review_ref: "scientific_review_missing_replay",
        metadata_bundle_ref: "metadata_bundle_missing_replay",
        license_record_ref: "license_record_missing_replay",
        measurement_provenance_ref: "measurement_provenance_missing_replay",
        access_approval_ref: "access_approval_missing_replay",
        citation_metadata_ref: "citation_metadata_missing_replay",
        release_approver_ref: "release_approver_missing_replay",
        ordinary_admin_ref: "ordinary_admin_missing_replay",
        unresolved_blocking_findings: 0,
        access_class: ResearchAccessClass::Controlled,
    })
    .unwrap()
}

#[test]
fn vanished_conflict_is_a_distinct_fail_closed_error() {
    let (mut client, schema_name) = isolated_client();
    let release = approved_release();

    let mut first = client.transaction().unwrap();
    assert_eq!(
        persist_approved_research_release(&mut first, &release).unwrap(),
        ResearchReleasePersistenceDisposition::Inserted
    );
    first.commit().unwrap();

    client
        .batch_execute(
            "ALTER TABLE research_release_approval \
                 DISABLE TRIGGER research_release_approval_immutable_guard;\
             CREATE OR REPLACE FUNCTION delete_conflicting_research_release_after_insert()\
             RETURNS trigger LANGUAGE plpgsql AS $$\
             BEGIN\
                 DELETE FROM research_release_approval\
                 WHERE research_release_ref = 'research_release_missing_replay';\
                 RETURN NULL;\
             END;\
             $$;\
             CREATE TRIGGER research_release_missing_replay_probe\
                 AFTER INSERT ON research_release_approval\
                 FOR EACH STATEMENT\
                 EXECUTE FUNCTION delete_conflicting_research_release_after_insert();",
        )
        .unwrap();

    let mut replay = client.transaction().unwrap();
    let error = persist_approved_research_release(&mut replay, &release)
        .expect_err("a vanished conflicting row must fail closed with explicit operator evidence");
    assert!(matches!(
        error,
        ResearchReleasePersistenceError::MissingStoredEvidence
    ));
    assert_eq!(
        error.to_string(),
        "stored research-release approval evidence disappeared during replay classification"
    );
    assert!(error.source().is_none());
    replay.rollback().unwrap();

    client
        .batch_execute(
            "DROP TRIGGER research_release_missing_replay_probe ON research_release_approval;\
             DROP FUNCTION delete_conflicting_research_release_after_insert();\
             ALTER TABLE research_release_approval \
                 ENABLE TRIGGER research_release_approval_immutable_guard;",
        )
        .unwrap();
    let row_count: i64 = client
        .query_one(
            "SELECT count(*) FROM research_release_approval \
             WHERE research_release_ref = 'research_release_missing_replay'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(row_count, 1, "rollback must preserve the original immutable evidence");

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}
