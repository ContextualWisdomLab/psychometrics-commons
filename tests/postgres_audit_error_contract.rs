//! Stable operator-facing error contracts for `PostgreSQL` audit persistence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::audit::{AuditEvidence, AuditEvidenceInput, AuditOutcome};
use psychometrics_commons_runtime::postgres_audit::{
    apply_audit_evidence_migration, load_audit_evidence, persist_audit_evidence,
    AuditPersistenceError,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn evidence_at(event_ref: &str, occurred_at_unix_ms: u64) -> AuditEvidence {
    AuditEvidence::new(AuditEvidenceInput {
        audit_event_ref: event_ref,
        tenant_ref: "tenant_research_alpha",
        actor_ref: "actor_publisher_alpha",
        purpose_code: "instrument_publication",
        action_code: "publish_instrument_release",
        resource_ref: "instrument_release_big_five_ko_v1",
        outcome: AuditOutcome::Failed,
        evidence_digest: DIGEST,
        occurred_at_unix_ms,
    })
    .unwrap()
}

#[test]
fn persistence_errors_expose_stable_messages_and_database_sources() {
    for (error, expected_message) in [
        (
            AuditPersistenceError::InvalidReference,
            "audit persistence references must be exact canonical opaque values",
        ),
        (
            AuditPersistenceError::ConflictingReplay,
            "audit event identity was replayed with conflicting immutable evidence",
        ),
        (
            AuditPersistenceError::CorruptHistory,
            "persisted audit evidence is inconsistent or unsupported",
        ),
        (
            AuditPersistenceError::TimestampOutOfRange,
            "audit event timestamp exceeds the supported PostgreSQL integer range",
        ),
        (
            AuditPersistenceError::UnsupportedIsolationLevel,
            "audit persistence requires read committed isolation",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }

    let mut client = test_client();
    let database_error = client
        .query_one("SELECT * FROM audit_error_contract_missing_relation", &[])
        .unwrap_err();
    let error = AuditPersistenceError::from(database_error);
    assert_eq!(error.to_string(), "PostgreSQL audit persistence failed");
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn persist_rejects_stronger_isolation_while_load_accepts_it() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_error_contract_test CASCADE;\
             CREATE SCHEMA audit_error_contract_test;\
             SET search_path TO audit_error_contract_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let overflow = evidence_at("audit_event_overflow_01", u64::MAX);
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_audit_evidence(&mut transaction, &overflow),
        Err(AuditPersistenceError::TimestampOutOfRange)
    ));
    transaction.rollback().unwrap();

    let failed = evidence_at("audit_event_failed_01", 1_785_000_000_000);
    let mut repeatable_write = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        persist_audit_evidence(&mut repeatable_write, &failed),
        Err(AuditPersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable_write.rollback().unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &failed).unwrap();
        transaction.commit().unwrap();
    }

    for isolation in [IsolationLevel::RepeatableRead, IsolationLevel::Serializable] {
        let mut stronger_read = client
            .build_transaction()
            .isolation_level(isolation)
            .start()
            .unwrap();
        let loaded = load_audit_evidence(
            &mut stronger_read,
            "tenant_research_alpha",
            "audit_event_failed_01",
        )
        .unwrap()
        .unwrap();
        assert_eq!(loaded.outcome(), AuditOutcome::Failed);
        stronger_read.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    for (tenant_ref, audit_event_ref) in [
        ("", "audit_event_failed_01"),
        ("tenant_research_alpha", ""),
        (" tenant_research_alpha ", "audit_event_failed_01"),
        ("tenant_research_alpha", "123"),
        ("tenant_research_alpha", "audit\u{2060}_event"),
    ] {
        assert!(matches!(
            load_audit_evidence(&mut transaction, tenant_ref, audit_event_ref),
            Err(AuditPersistenceError::InvalidReference)
        ));
    }
    transaction.rollback().unwrap();
}

fn prepare_unconstrained_audit_schema(client: &mut Client, schema: &str) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE;\
             CREATE SCHEMA {schema};\
             SET search_path TO {schema};"
        ))
        .unwrap();
    apply_audit_evidence_migration(client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_occurrence_positive_check;\
             ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_digest_shape_check;\
             ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_purpose_code_shape_check;",
        )
        .unwrap();
}

fn insert_corrupt_audit_row(
    client: &mut Client,
    audit_event_ref: &str,
    purpose_code: &str,
    evidence_digest: &str,
    occurred_at_unix_ms: i64,
) {
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &audit_event_ref,
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &purpose_code,
                &"publish_instrument_release",
                &"instrument_release_big_five_ko_v1",
                &"succeeded",
                &evidence_digest,
                &occurred_at_unix_ms,
            ],
        )
        .unwrap();
}

fn load_must_report_corrupt_history(client: &mut Client, audit_event_ref: &str) {
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_audit_evidence(&mut transaction, "tenant_research_alpha", audit_event_ref),
        Err(AuditPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn load_fails_closed_on_corrupt_stored_fields() {
    let mut client = test_client();
    for (schema, event_ref, purpose, digest, time) in [
        (
            "audit_corrupt_time_test",
            "audit_event_corrupt_time_01",
            "instrument_publication",
            DIGEST,
            -1_i64,
        ),
        (
            "audit_corrupt_digest_test",
            "audit_event_corrupt_digest_01",
            "instrument_publication",
            "sha256:not-a-canonical-digest",
            1_785_000_000_000_i64,
        ),
        (
            "audit_corrupt_purpose_test",
            "audit_event_corrupt_purpose_01",
            "HasUppercase",
            DIGEST,
            1_785_000_000_000_i64,
        ),
    ] {
        prepare_unconstrained_audit_schema(&mut client, schema);
        insert_corrupt_audit_row(&mut client, event_ref, purpose, digest, time);
        load_must_report_corrupt_history(&mut client, event_ref);
    }
}

#[test]
fn aborted_public_persistence_maps_to_database_error() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_aborted_tx_test CASCADE;\
             CREATE SCHEMA audit_aborted_tx_test;\
             SET search_path TO audit_aborted_tx_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();
    let evidence = evidence_at("audit_event_aborted_01", 1_785_000_000_000);
    let mut transaction = client.transaction().unwrap();
    assert!(transaction
        .batch_execute("SELECT 1 FROM audit_aborted_missing_relation")
        .is_err());
    assert!(matches!(
        persist_audit_evidence(&mut transaction, &evidence),
        Err(AuditPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
