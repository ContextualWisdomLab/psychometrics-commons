//! Real `PostgreSQL` contract for append-only, tenant-scoped audit evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::audit::{AuditEvidence, AuditEvidenceInput, AuditOutcome};
use psychometrics_commons_runtime::postgres_audit::{
    apply_audit_evidence_migration, load_audit_evidence, persist_audit_evidence,
    AuditPersistenceDisposition, AuditPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static AUDIT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn audit_test_guard() -> MutexGuard<'static, ()> {
    AUDIT_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS audit_evidence_persistence_test;\
             SET search_path TO audit_evidence_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_audit_objects(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS audit_evidence_persistence_test.audit_evidence_record CASCADE;\
             DROP FUNCTION IF EXISTS audit_evidence_persistence_test.reject_audit_evidence_mutation() CASCADE;",
        )
        .unwrap();
}

fn evidence(
    event_ref: &str,
    tenant_ref: &str,
    actor_ref: &str,
    outcome: AuditOutcome,
    digest: &str,
) -> AuditEvidence {
    AuditEvidence::new(AuditEvidenceInput {
        audit_event_ref: event_ref,
        tenant_ref,
        actor_ref,
        purpose_code: "instrument_publication",
        action_code: "publish_instrument_release",
        resource_ref: "instrument_release_big_five_ko_v1",
        outcome,
        evidence_digest: digest,
        occurred_at_unix_ms: 1_785_000_000_000,
    })
    .unwrap()
}

#[test]
fn exact_replay_is_idempotent_and_identity_rebinding_fails_closed() {
    let _guard = audit_test_guard();
    let mut client = test_client();
    reset_audit_objects(&mut client);
    apply_audit_evidence_migration(&mut client).unwrap();

    let first = evidence(
        "audit_event_publish_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_audit_evidence(&mut transaction, &first).unwrap(),
            AuditPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_audit_evidence(&mut transaction, &first).unwrap(),
            AuditPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let rebound = evidence(
        "audit_event_publish_01",
        "tenant_research_alpha",
        "actor_publisher_beta",
        AuditOutcome::Succeeded,
        OTHER_DIGEST,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_audit_evidence(&mut transaction, &rebound),
        Err(AuditPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn tenant_scoped_reload_returns_exact_evidence_without_cross_tenant_existence_leak() {
    let _guard = audit_test_guard();
    let mut client = test_client();
    reset_audit_objects(&mut client);
    apply_audit_evidence_migration(&mut client).unwrap();

    let first = evidence(
        "audit_event_denied_01",
        "tenant_research_alpha",
        "actor_researcher_alpha",
        AuditOutcome::Denied,
        DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    let loaded = load_audit_evidence(
        &mut transaction,
        "tenant_research_alpha",
        "audit_event_denied_01",
    )
    .unwrap()
    .expect("same-tenant audit evidence must reload");
    assert_eq!(loaded, first);
    assert!(load_audit_evidence(
        &mut transaction,
        "tenant_research_beta",
        "audit_event_denied_01"
    )
    .unwrap()
    .is_none());
    transaction.rollback().unwrap();
}

#[test]
fn database_rejects_update_delete_and_truncate_of_audit_history() {
    let _guard = audit_test_guard();
    let mut client = test_client();
    reset_audit_objects(&mut client);
    apply_audit_evidence_migration(&mut client).unwrap();
    let first = evidence(
        "audit_event_immutable_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }

    for statement in [
        "UPDATE audit_evidence_record SET actor_ref = 'actor_attacker_alpha' WHERE audit_event_ref = 'audit_event_immutable_01'",
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_immutable_01'",
        "TRUNCATE TABLE audit_evidence_record",
    ] {
        let error = client.execute(statement, &[]).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("audit evidence is append-only"),
            "unexpected mutation error: {message}"
        );
    }

    assert_eq!(
        client
            .query_one("SELECT count(*) FROM audit_evidence_record", &[])
            .unwrap()
            .get::<_, i64>(0),
        1
    );
}

#[test]
fn unsupported_isolation_and_corrupt_stored_outcome_fail_closed() {
    let _guard = audit_test_guard();
    let mut client = test_client();
    reset_audit_objects(&mut client);
    apply_audit_evidence_migration(&mut client).unwrap();
    let first = evidence(
        "audit_event_isolation_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_audit_evidence(&mut transaction, &first),
        Err(AuditPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute(
            "ALTER TABLE audit_evidence_record\
             DROP CONSTRAINT audit_evidence_outcome_allowed_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &"audit_event_corrupt_01",
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &"instrument_publication",
                &"publish_instrument_release",
                &"instrument_release_big_five_ko_v1",
                &"unexpected",
                &DIGEST,
                &1_785_000_000_000_i64,
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_audit_evidence(
            &mut transaction,
            "tenant_research_alpha",
            "audit_event_corrupt_01"
        ),
        Err(AuditPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn migration_is_idempotent_and_database_constraints_reject_bad_machine_evidence() {
    let _guard = audit_test_guard();
    let mut client = test_client();
    reset_audit_objects(&mut client);
    apply_audit_evidence_migration(&mut client).unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let invalid_rows = [
        "('audit_event_bad_purpose', 'tenant_research_alpha', 'actor_publisher_alpha', 'HasUppercase', 'publish_instrument_release', 'instrument_release_big_five_ko_v1', 'succeeded', 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 1785000000000)",
        "('audit_event_bad_action', 'tenant_research_alpha', 'actor_publisher_alpha', 'instrument_publication', 'has-hyphen', 'instrument_release_big_five_ko_v1', 'succeeded', 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 1785000000000)",
        "('audit_event_bad_outcome', 'tenant_research_alpha', 'actor_publisher_alpha', 'instrument_publication', 'publish_instrument_release', 'instrument_release_big_five_ko_v1', 'unknown', 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', 1785000000000)",
        "('audit_event_bad_digest', 'tenant_research_alpha', 'actor_publisher_alpha', 'instrument_publication', 'publish_instrument_release', 'instrument_release_big_five_ko_v1', 'succeeded', 'sha256:deadbeef', 1785000000000)",
    ];
    for row in invalid_rows {
        let statement = format!(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES {row}"
        );
        assert!(
            client.execute(&statement, &[]).is_err(),
            "row must fail closed: {row}"
        );
    }

    assert_eq!(
        client
            .query_one("SELECT count(*) FROM audit_evidence_record", &[])
            .unwrap()
            .get::<_, i64>(0),
        0
    );
}
