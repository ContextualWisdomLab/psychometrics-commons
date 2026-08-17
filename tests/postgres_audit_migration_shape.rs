//! Migration-shape contract for durable product audit evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

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
    let message = error
        .as_db_error()
        .map_or_else(|| error.to_string(), |database| database.message().to_owned());
    assert!(
        message.contains("audit_evidence_record") && message.contains("contract"),
        "migration must identify owned-schema drift instead of silently accepting it: {message}"
    );
}
