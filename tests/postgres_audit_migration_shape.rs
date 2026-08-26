//! Migration-order and fail-closed shape contracts for durable audit evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;
use psychometrics_commons_runtime::postgres_audit_retention::apply_audit_evidence_retention_migration;

const AUDIT_SCHEMA_MIGRATION: &str = include_str!("../migrations/0040_audit_evidence_record.sql");

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn core_migration_serializes_creation_before_observing_relation_state() {
    let begin = AUDIT_SCHEMA_MIGRATION
        .find("BEGIN\n")
        .expect("migration DO block must have an executable body");
    let lock = AUDIT_SCHEMA_MIGRATION
        .find("PERFORM pg_advisory_xact_lock(hashtext('psychometrics-commons:migration-0040'));")
        .expect("core migration must serialize concurrent first creation");
    let first_relation_observation = AUDIT_SCHEMA_MIGRATION
        .find("relation_ref := to_regclass('audit_evidence_record');")
        .expect("core migration must inspect its owned table");

    assert!(lock > begin);
    assert!(
        first_relation_observation > lock,
        "owned relation state must be observed only after acquiring the migration lock"
    );
}

#[test]
fn core_migration_rejects_preexisting_relation_with_wrong_owned_schema() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_shape_test CASCADE;\
             CREATE SCHEMA audit_migration_shape_test;\
             SET search_path TO audit_migration_shape_test;\
             CREATE TABLE audit_evidence_record (\
                 audit_event_ref TEXT NOT NULL,\
                 tenant_ref TEXT NOT NULL\
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
fn both_migrations_apply_inside_the_caller_transaction() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_transaction_test CASCADE;\
             CREATE SCHEMA audit_migration_transaction_test;\
             SET search_path TO audit_migration_transaction_test;",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    apply_audit_evidence_migration(&mut transaction).unwrap();
    apply_audit_evidence_retention_migration(&mut transaction).unwrap();
    transaction.commit().unwrap();

    let (table_count, routine_count, security_definer): (i64, i64, bool) = client
        .query_one(
            "SELECT\
                 (SELECT count(*)::bigint FROM information_schema.tables\
                  WHERE table_schema = 'audit_migration_transaction_test'\
                    AND table_name = 'audit_evidence_record'),\
                 (SELECT count(*)::bigint FROM information_schema.routines\
                  WHERE routine_schema = 'audit_migration_transaction_test'\
                    AND routine_name = 'expire_audit_evidence_before'),\
                 (SELECT prosecdef FROM pg_proc\
                  WHERE oid = 'audit_migration_transaction_test.expire_audit_evidence_before(bigint)'::regprocedure)",
            &[],
        )
        .unwrap()
        .get::<_, (i64, i64, bool)>(0);
    assert_eq!(table_count, 1);
    assert_eq!(routine_count, 1);
    assert!(security_definer, "bounded retention must execute under its migration owner");
}

#[test]
fn retention_migration_fails_closed_when_core_audit_table_is_absent() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_retention_without_core_test CASCADE;\
             CREATE SCHEMA audit_retention_without_core_test;\
             SET search_path TO audit_retention_without_core_test;",
        )
        .unwrap();

    let error = apply_audit_evidence_retention_migration(&mut client)
        .expect_err("retention must never create or infer a missing core audit table");
    let message = error.as_db_error().map_or_else(
        || error.to_string(),
        |database| database.message().to_owned(),
    );
    assert!(message.contains("must exist before migration 0041"));
}
