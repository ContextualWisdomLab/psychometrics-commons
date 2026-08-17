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
fn persist_and_load_fail_closed_on_range_isolation_and_caller_aliases() {
    let mut client = test_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS audit_error_contract_test;\
             SET search_path TO audit_error_contract_test;\
             DROP TABLE IF EXISTS audit_evidence_record CASCADE;\
             DROP FUNCTION IF EXISTS reject_audit_evidence_mutation() CASCADE;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let overflow = evidence_at("audit_event_overflow_01", u64::MAX);
    let mut transaction = client.transaction().unwrap();
    let overflow_error = persist_audit_evidence(&mut transaction, &overflow)
        .expect_err("timestamps above the PostgreSQL BIGINT range must fail closed");
    assert!(matches!(
        overflow_error,
        AuditPersistenceError::TimestampOutOfRange
    ));
    transaction.rollback().unwrap();

    let failed = evidence_at("audit_event_failed_01", 1_785_000_000_000);
    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    let isolation_error = persist_audit_evidence(&mut serializable, &failed)
        .expect_err("non-read-committed audit writes must fail closed");
    assert!(matches!(
        isolation_error,
        AuditPersistenceError::UnsupportedIsolationLevel
    ));
    serializable.rollback().unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &failed).unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    let loaded = load_audit_evidence(
        &mut transaction,
        "tenant_research_alpha",
        "audit_event_failed_01",
    )
    .unwrap()
    .expect("failed outcomes must reload as durable audit evidence");
    assert_eq!(loaded.outcome(), AuditOutcome::Failed);

    for (tenant_ref, audit_event_ref) in [
        ("", "audit_event_failed_01"),
        ("tenant_research_alpha", ""),
        (" tenant_research_alpha ", "audit_event_failed_01"),
        ("tenant_research_alpha", "123"),
    ] {
        let error = load_audit_evidence(&mut transaction, tenant_ref, audit_event_ref)
            .expect_err("load aliases must fail closed before a database lookup");
        assert!(matches!(error, AuditPersistenceError::InvalidReference));
    }
    transaction.rollback().unwrap();
}

#[test]
fn load_fails_closed_on_corrupt_stored_timestamp_and_digest() {
    let mut client = test_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS audit_corrupt_history_test;\
             SET search_path TO audit_corrupt_history_test;\
             DROP TABLE IF EXISTS audit_evidence_record CASCADE;\
             DROP FUNCTION IF EXISTS reject_audit_evidence_mutation() CASCADE;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_occurrence_positive_check;\
             ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_digest_shape_check;",
        )
        .unwrap();

    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &"audit_event_corrupt_time_01",
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &"instrument_publication",
                &"publish_instrument_release",
                &"instrument_release_big_five_ko_v1",
                &"succeeded",
                &DIGEST,
                &-1_i64,
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &"audit_event_corrupt_digest_01",
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &"instrument_publication",
                &"publish_instrument_release",
                &"instrument_release_big_five_ko_v1",
                &"succeeded",
                &"sha256:not-a-canonical-digest",
                &1_785_000_000_000_i64,
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let time_error = load_audit_evidence(
        &mut transaction,
        "tenant_research_alpha",
        "audit_event_corrupt_time_01",
    )
    .expect_err("negative stored event time must fail closed");
    assert!(matches!(time_error, AuditPersistenceError::CorruptHistory));
    let digest_error = load_audit_evidence(
        &mut transaction,
        "tenant_research_alpha",
        "audit_event_corrupt_digest_01",
    )
    .expect_err("noncanonical stored digest must fail closed");
    assert!(matches!(
        digest_error,
        AuditPersistenceError::CorruptHistory
    ));
    transaction.rollback().unwrap();
}
