//! Real `PostgreSQL` contract for immutable approved research-release evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_research_release::{
    apply_research_release_migration, persist_approved_research_release,
    ResearchReleasePersistenceDisposition, ResearchReleasePersistenceError,
};
use psychometrics_commons_runtime::research_release::{
    approve_research_release, ApprovedResearchRelease, ResearchAccessClass,
    ResearchReleaseCandidate,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TAMPERABLE_COLUMNS: [&str; 13] = [
    "dataset_snapshot_ref",
    "research_scope_ref",
    "manifest_digest",
    "privacy_review_ref",
    "scientific_review_ref",
    "metadata_bundle_ref",
    "license_record_ref",
    "measurement_provenance_ref",
    "access_approval_ref",
    "citation_metadata_ref",
    "release_approver_ref",
    "ordinary_admin_ref",
    "access_class",
];
static RESEARCH_RELEASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    RESEARCH_RELEASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS research_release_persistence_test;\
             SET search_path TO research_release_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_table(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS research_release_approval;")
        .unwrap();
}

fn inject_stored_tamper(client: &mut Client, column: &str, value: &str, release_ref: &str) {
    assert!(
        TAMPERABLE_COLUMNS.contains(&column),
        "tamper injection must target a known immutable evidence column"
    );
    client
        .batch_execute(
            "ALTER TABLE research_release_approval \
             DISABLE TRIGGER research_release_approval_immutable_guard;",
        )
        .unwrap();
    let mutation = client.execute(
        &format!(
            "UPDATE research_release_approval SET {column} = $1 WHERE research_release_ref = $2"
        ),
        &[&value, &release_ref],
    );
    client
        .batch_execute(
            "ALTER TABLE research_release_approval \
             ENABLE TRIGGER research_release_approval_immutable_guard;",
        )
        .unwrap();
    mutation.expect("test-only tamper injection must satisfy all non-immutability constraints");
}

fn approved_release(
    release_ref: &str,
    manifest_digest: &str,
    access_class: ResearchAccessClass,
    metadata_bundle_ref: &str,
) -> ApprovedResearchRelease {
    approve_research_release(ResearchReleaseCandidate {
        release_ref,
        dataset_snapshot_ref: "dataset_snapshot_research_alpha",
        research_scope_ref: "research_scope_research_alpha",
        manifest_digest,
        privacy_review_ref: "privacy_review_research_alpha",
        scientific_review_ref: "scientific_review_research_alpha",
        metadata_bundle_ref,
        license_record_ref: "license_record_research_alpha",
        measurement_provenance_ref: "measurement_provenance_research_alpha",
        access_approval_ref: "access_approval_research_alpha",
        citation_metadata_ref: "citation_metadata_research_alpha",
        release_approver_ref: "release_approver_research_alpha",
        ordinary_admin_ref: "ordinary_admin_research_alpha",
        unresolved_blocking_findings: 0,
        access_class,
    })
    .unwrap()
}

fn persist_ok(
    client: &mut Client,
    release: &ApprovedResearchRelease,
) -> ResearchReleasePersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_approved_research_release(&mut transaction, release).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    release: &ApprovedResearchRelease,
) -> ResearchReleasePersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_approved_research_release(&mut transaction, release).unwrap_err();
    transaction.rollback().unwrap();
    error
}

#[test]
fn every_access_class_persists_and_exact_replay_is_idempotent() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();

    let cases = [
        (
            "research_release_public",
            ResearchAccessClass::Public,
            "public",
        ),
        (
            "research_release_controlled",
            ResearchAccessClass::Controlled,
            "controlled",
        ),
        (
            "research_release_private",
            ResearchAccessClass::Private,
            "private",
        ),
        (
            "research_release_embargoed",
            ResearchAccessClass::Embargoed,
            "embargoed",
        ),
    ];
    for (release_ref, access_class, expected_access_class) in cases {
        let release = approved_release(
            release_ref,
            DIGEST_A,
            access_class,
            "metadata_bundle_research_alpha",
        );
        assert_eq!(
            persist_ok(&mut client, &release),
            ResearchReleasePersistenceDisposition::Inserted
        );
        assert_eq!(
            persist_ok(&mut client, &release),
            ResearchReleasePersistenceDisposition::Duplicate
        );
        let stored_access_class: String = client
            .query_one(
                "SELECT access_class FROM research_release_approval WHERE research_release_ref = $1",
                &[&release_ref],
            )
            .unwrap()
            .get(0);
        assert_eq!(stored_access_class, expected_access_class);
    }
}

#[test]
fn immutable_rebinding_fails_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();

    let original = approved_release(
        "research_release_conflict",
        DIGEST_A,
        ResearchAccessClass::Controlled,
        "metadata_bundle_research_alpha",
    );
    assert_eq!(
        persist_ok(&mut client, &original),
        ResearchReleasePersistenceDisposition::Inserted
    );

    let digest_rebind = approved_release(
        "research_release_conflict",
        DIGEST_B,
        ResearchAccessClass::Controlled,
        "metadata_bundle_research_alpha",
    );
    assert!(matches!(
        persist_err(&mut client, &digest_rebind),
        ResearchReleasePersistenceError::ConflictingReplay
    ));

    let metadata_rebind = approved_release(
        "research_release_conflict",
        DIGEST_A,
        ResearchAccessClass::Controlled,
        "metadata_bundle_research_other",
    );
    assert!(matches!(
        persist_err(&mut client, &metadata_rebind),
        ResearchReleasePersistenceError::ConflictingReplay
    ));
}

#[test]
fn every_persisted_approval_field_is_part_of_the_immutable_replay_identity() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();

    let release = approved_release(
        "research_release_field_identity",
        DIGEST_A,
        ResearchAccessClass::Controlled,
        "metadata_bundle_research_alpha",
    );
    assert_eq!(
        persist_ok(&mut client, &release),
        ResearchReleasePersistenceDisposition::Inserted
    );

    let cases = [
        (
            "dataset_snapshot_ref",
            "dataset_snapshot_research_other",
            "dataset_snapshot_research_alpha",
        ),
        (
            "research_scope_ref",
            "research_scope_research_other",
            "research_scope_research_alpha",
        ),
        ("manifest_digest", DIGEST_B, DIGEST_A),
        (
            "privacy_review_ref",
            "privacy_review_research_other",
            "privacy_review_research_alpha",
        ),
        (
            "scientific_review_ref",
            "scientific_review_research_other",
            "scientific_review_research_alpha",
        ),
        (
            "metadata_bundle_ref",
            "metadata_bundle_research_other",
            "metadata_bundle_research_alpha",
        ),
        (
            "license_record_ref",
            "license_record_research_other",
            "license_record_research_alpha",
        ),
        (
            "measurement_provenance_ref",
            "measurement_provenance_research_other",
            "measurement_provenance_research_alpha",
        ),
        (
            "access_approval_ref",
            "access_approval_research_other",
            "access_approval_research_alpha",
        ),
        (
            "citation_metadata_ref",
            "citation_metadata_research_other",
            "citation_metadata_research_alpha",
        ),
        (
            "release_approver_ref",
            "release_approver_research_other",
            "release_approver_research_alpha",
        ),
        (
            "ordinary_admin_ref",
            "ordinary_admin_research_other",
            "ordinary_admin_research_alpha",
        ),
        ("access_class", "private", "controlled"),
    ];

    for (column, tampered_value, original_value) in cases {
        inject_stored_tamper(&mut client, column, tampered_value, release.release_ref());
        assert!(matches!(
            persist_err(&mut client, &release),
            ResearchReleasePersistenceError::ConflictingReplay
        ));
        inject_stored_tamper(&mut client, column, original_value, release.release_ref());
    }

    assert_eq!(
        persist_ok(&mut client, &release),
        ResearchReleasePersistenceDisposition::Duplicate,
        "restoring every immutable field must recover exact-replay identity"
    );
}

#[test]
fn stored_evidence_tampering_is_detected_on_replay_even_if_database_guard_is_bypassed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();

    let release = approved_release(
        "research_release_tamper",
        DIGEST_A,
        ResearchAccessClass::Public,
        "metadata_bundle_research_alpha",
    );
    persist_ok(&mut client, &release);
    inject_stored_tamper(
        &mut client,
        "scientific_review_ref",
        "scientific_review_research_other",
        release.release_ref(),
    );
    assert!(matches!(
        persist_err(&mut client, &release),
        ResearchReleasePersistenceError::ConflictingReplay
    ));
}

#[test]
fn unsupported_isolation_fails_before_write() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();
    let release = approved_release(
        "research_release_serializable",
        DIGEST_A,
        ResearchAccessClass::Private,
        "metadata_bundle_research_alpha",
    );

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_approved_research_release(&mut transaction, &release),
        Err(ResearchReleasePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    let count: i64 = client
        .query_one("SELECT count(*) FROM research_release_approval", &[])
        .unwrap()
        .get(0);
    assert_eq!(count, 0);
}

#[test]
fn database_failure_is_typed_and_exposes_the_postgres_source() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_table(&mut client);
    apply_research_release_migration(&mut client).unwrap();
    client
        .batch_execute("DROP TABLE research_release_approval")
        .unwrap();
    let release = approved_release(
        "research_release_database_error",
        DIGEST_A,
        ResearchAccessClass::Embargoed,
        "metadata_bundle_research_alpha",
    );

    let error = persist_err(&mut client, &release);
    assert!(matches!(
        error,
        ResearchReleasePersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL research-release persistence failed"
    );
    assert!(error.source().is_some());
}

#[test]
fn non_database_errors_have_operator_facing_display_without_sources() {
    let errors = [
        ResearchReleasePersistenceError::UnsupportedIsolationLevel,
        ResearchReleasePersistenceError::ConflictingReplay,
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_none());
    }
}
