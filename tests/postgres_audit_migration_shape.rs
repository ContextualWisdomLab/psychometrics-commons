//! Migration-shape contract for durable product audit evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

const AUDIT_EVIDENCE_MIGRATION: &str = include_str!("../migrations/0040_audit_evidence_record.sql");

#[test]
fn migration_serializes_creation_before_observing_relation_state() {
    let begin = AUDIT_EVIDENCE_MIGRATION
        .find("BEGIN\n")
        .expect("migration DO block must have an executable body");
    let lock = AUDIT_EVIDENCE_MIGRATION
        .find("PERFORM pg_advisory_xact_lock(hashtext('psychometrics-commons:migration-0040'));")
        .expect(
            "migration must serialize concurrent first creation with a transaction advisory lock",
        );
    let relation_refresh = AUDIT_EVIDENCE_MIGRATION
        .find("relation_ref := to_regclass('audit_evidence_record');")
        .expect("migration must observe owned relation state after acquiring the lock");
    let created_table_refresh = AUDIT_EVIDENCE_MIGRATION
        .find("created_table := relation_ref IS NULL;")
        .expect("migration must derive creation state from the post-lock relation observation");
    let first_relation_observation = AUDIT_EVIDENCE_MIGRATION
        .find("to_regclass('audit_evidence_record')")
        .expect("migration must inspect the owned relation");

    assert!(
        lock > begin,
        "advisory lock must execute inside the migration DO block"
    );
    assert!(
        first_relation_observation > lock,
        "no relation observation may occur before the migration advisory lock"
    );
    assert!(
        relation_refresh > lock && created_table_refresh > relation_refresh,
        "relation and creation state must be refreshed only after lock acquisition"
    );
}

#[test]
fn migration_rejects_preexisting_relation_with_wrong_owned_schema() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_shape_test CASCADE;\
             CREATE SCHEMA audit_migration_shape_test;\
             SET search_path TO audit_migration_shape_test;\
             CREATE TABLE audit_evidence_record (\
                 audit_event_ref TEXT NOT NULL,\
                 tenant_ref TEXT NOT NULL,\
                 actor_ref TEXT NOT NULL,\
                 purpose_code TEXT NOT NULL,\
                 action_code TEXT NOT NULL,\
                 resource_ref TEXT NOT NULL,\
                 outcome_code TEXT NOT NULL,\
                 evidence_digest TEXT NOT NULL,\
                 occurred_at_unix_ms BIGINT NOT NULL,\
                 recorded_at TIMESTAMPTZ NOT NULL\
             );",
        )
        .unwrap();

    let error = apply_audit_evidence_migration(&mut client)
        .expect_err("migration must reject a preexisting relation it does not own exactly");
    let message = error.as_db_error().map_or_else(
        || error.to_string(),
        |database| database.message().to_owned(),
    );
    assert!(
        message.contains("audit_evidence_record") && message.contains("contract"),
        "migration must identify owned-schema drift instead of silently accepting it: {message}"
    );
}

#[test]
fn migration_applies_inside_the_caller_transaction() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_transaction_test CASCADE;\
             CREATE SCHEMA audit_migration_transaction_test;\
             SET search_path TO audit_migration_transaction_test;",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    apply_audit_evidence_migration(&mut transaction)
        .expect("audit migration must apply inside the caller transaction");
    transaction.commit().unwrap();

    let count: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM information_schema.tables \
             WHERE table_schema = 'audit_migration_transaction_test' \
               AND table_name = 'audit_evidence_record'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
}
