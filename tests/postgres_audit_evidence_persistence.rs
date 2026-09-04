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

fn guard() -> MutexGuard<'static, ()> {
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
            "DROP SCHEMA IF EXISTS audit_evidence_persistence_test CASCADE;\
             CREATE SCHEMA audit_evidence_persistence_test;\
             SET search_path TO audit_evidence_persistence_test;",
        )
        .unwrap();
    client
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
fn exact_replay_is_idempotent_and_conflicting_replay_fails_closed() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();
    let original = evidence(
        "audit_event_publish_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_audit_evidence(&mut transaction, &original).unwrap(),
            AuditPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_audit_evidence(&mut transaction, &original).unwrap(),
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
fn tenant_scoped_reload_does_not_disclose_cross_tenant_existence() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();
    let denied = evidence(
        "audit_event_denied_01",
        "tenant_research_alpha",
        "actor_researcher_alpha",
        AuditOutcome::Denied,
        DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &denied).unwrap();
        transaction.commit().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        load_audit_evidence(
            &mut transaction,
            "tenant_research_alpha",
            "audit_event_denied_01"
        )
        .unwrap(),
        Some(denied)
    );
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
fn caller_identity_aliases_and_write_isolation_fail_before_persistence() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_audit_evidence(
            &mut transaction,
            " tenant_research_alpha ",
            "audit_event_missing"
        ),
        Err(AuditPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();

    let record = evidence(
        "audit_event_serializable_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );
    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_audit_evidence(&mut serializable, &record),
        Err(AuditPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();
}

#[test]
fn database_blocks_application_update_delete_and_truncate() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();
    let record = evidence(
        "audit_event_immutable_01",
        "tenant_research_alpha",
        "actor_publisher_alpha",
        AuditOutcome::Succeeded,
        DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_audit_evidence(&mut transaction, &record).unwrap();
        transaction.commit().unwrap();
    }

    for statement in [
        "UPDATE audit_evidence_record SET actor_ref = 'actor_attacker' WHERE audit_event_ref = 'audit_event_immutable_01'",
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_immutable_01'",
        "TRUNCATE TABLE audit_evidence_record",
    ] {
        let error = client
            .execute(statement, &[])
            .expect_err("append-only history must reject application mutation");
        let message = error
            .as_db_error()
            .map_or_else(|| error.to_string(), |database| database.message().to_owned());
        assert!(message.contains("audit evidence is append-only"));
    }
}

#[test]
fn corrupt_stored_outcome_fails_closed_on_reload() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE audit_evidence_record DROP CONSTRAINT audit_evidence_outcome_allowed_check;",
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
fn migration_is_idempotent_and_rejects_invalid_machine_fields() {
    let _guard = guard();
    let mut client = test_client();
    apply_audit_evidence_migration(&mut client).unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    for (purpose, action, outcome, digest) in [
        (
            "HasUppercase",
            "publish_instrument_release",
            "succeeded",
            DIGEST,
        ),
        ("instrument_publication", "has-hyphen", "succeeded", DIGEST),
        (
            "instrument_publication",
            "publish_instrument_release",
            "unknown",
            DIGEST,
        ),
        (
            "instrument_publication",
            "publish_instrument_release",
            "succeeded",
            "sha256:deadbeef",
        ),
    ] {
        assert!(client
            .execute(
                "INSERT INTO audit_evidence_record (\
                    audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                    outcome_code, evidence_digest, occurred_at_unix_ms\
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                &[
                    &format!("audit_event_invalid_{purpose}_{action}_{outcome}"),
                    &"tenant_research_alpha",
                    &"actor_publisher_alpha",
                    &purpose,
                    &action,
                    &"instrument_release_big_five_ko_v1",
                    &outcome,
                    &digest,
                    &1_785_000_000_000_i64,
                ],
            )
            .is_err());
    }
}
