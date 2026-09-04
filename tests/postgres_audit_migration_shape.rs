//! Migration-order and fail-closed shape contracts for durable audit evidence.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;
use psychometrics_commons_runtime::postgres_audit_retention::apply_audit_evidence_retention_migration;

const AUDIT_SCHEMA_MIGRATION: &str = include_str!("../migrations/0040_audit_evidence_record.sql");
const AUDIT_EVIDENCE_OWNER_ROLE: &str = "psychometrics_audit_evidence_owner";

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
fn core_migration_uses_dedicated_nologin_owner_without_runtime_set_paths() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_owner_shape_test CASCADE; \
             CREATE SCHEMA audit_owner_shape_test; \
             SET search_path TO audit_owner_shape_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let owner = client
        .query_opt(
            "SELECT \
                 owner_role.rolcanlogin, \
                 owner_role.rolsuper, \
                 owner_role.rolcreatedb, \
                 owner_role.rolcreaterole, \
                 owner_role.rolreplication, \
                 owner_role.rolbypassrls, \
                 pg_get_userbyid(table_record.relowner), \
                 pg_get_userbyid(reference_function.proowner), \
                 pg_get_userbyid(mutation_function.proowner), \
                 EXISTS (\
                     SELECT 1 \
                     FROM pg_roles AS login_role \
                     WHERE login_role.rolcanlogin \
                       AND NOT login_role.rolsuper \
                       AND (\
                           pg_has_role(login_role.oid, owner_role.oid, 'SET') \
                           OR pg_has_role(login_role.oid, owner_role.oid, 'USAGE') \
                       ) \
                 ) \
             FROM pg_roles AS owner_role \
             JOIN pg_class AS table_record \
               ON table_record.oid = 'audit_owner_shape_test.audit_evidence_record'::regclass \
             JOIN pg_proc AS reference_function \
               ON reference_function.oid = \
                  'audit_owner_shape_test.audit_evidence_reference_is_valid(text)'::regprocedure \
             JOIN pg_proc AS mutation_function \
               ON mutation_function.oid = \
                  'audit_owner_shape_test.reject_audit_evidence_mutation()'::regprocedure \
             WHERE owner_role.rolname = $1",
            &[&AUDIT_EVIDENCE_OWNER_ROLE],
        )
        .unwrap()
        .expect("audit migration must provision its dedicated owner role");

    for attribute_index in 0..6 {
        assert!(
            !owner.get::<_, bool>(attribute_index),
            "audit owner must remain NOLOGIN and free of cluster-escalation attributes"
        );
    }
    assert_eq!(owner.get::<_, String>(6), AUDIT_EVIDENCE_OWNER_ROLE);
    assert_eq!(owner.get::<_, String>(7), AUDIT_EVIDENCE_OWNER_ROLE);
    assert_eq!(owner.get::<_, String>(8), AUDIT_EVIDENCE_OWNER_ROLE);
    assert!(
        !owner.get::<_, bool>(9),
        "no non-superuser login role may inherit or SET ROLE into the audit owner"
    );
}

#[test]
fn explicit_runtime_grant_enables_append_and_read_without_mutation_authority() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_runtime_grant_test CASCADE; \
             CREATE SCHEMA audit_runtime_grant_test; \
             SET search_path TO audit_runtime_grant_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let backend_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    let runtime_role = format!("audit_runtime_shape_{}_{}", std::process::id(), backend_pid);
    client
        .batch_execute(&format!(
            "CREATE ROLE {runtime_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS; \
             GRANT USAGE ON SCHEMA audit_runtime_grant_test TO {runtime_role}; \
             GRANT SELECT, INSERT ON TABLE audit_runtime_grant_test.audit_evidence_record TO {runtime_role};"
        ))
        .expect("deployment-selected runtime role must accept the documented least-privilege grants");

    let privileges = client
        .query_one(
            "SELECT \
                 has_schema_privilege($1, 'audit_runtime_grant_test', 'USAGE'), \
                 has_table_privilege($1, 'audit_runtime_grant_test.audit_evidence_record', 'SELECT'), \
                 has_table_privilege($1, 'audit_runtime_grant_test.audit_evidence_record', 'INSERT'), \
                 has_table_privilege($1, 'audit_runtime_grant_test.audit_evidence_record', 'UPDATE'), \
                 has_table_privilege($1, 'audit_runtime_grant_test.audit_evidence_record', 'DELETE'), \
                 has_table_privilege($1, 'audit_runtime_grant_test.audit_evidence_record', 'TRUNCATE')",
            &[&runtime_role],
        )
        .unwrap();
    assert!(privileges.get::<_, bool>(0));
    assert!(privileges.get::<_, bool>(1));
    assert!(privileges.get::<_, bool>(2));
    for privilege_index in 3..6 {
        assert!(
            !privileges.get::<_, bool>(privilege_index),
            "runtime audit role must not receive mutation privileges"
        );
    }

    client
        .batch_execute(&format!(
            "SET ROLE {runtime_role}; SET search_path TO audit_runtime_grant_test;"
        ))
        .unwrap();
    client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref, \
                outcome_code, evidence_digest, occurred_at_unix_ms \
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &"audit_event_runtime_grant_01",
                &"tenant_runtime_alpha",
                &"actor_runtime_alpha",
                &"runtime_audit",
                &"record_audit_evidence",
                &"resource_runtime_alpha",
                &"succeeded",
                &"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                &1_785_000_000_000_i64,
            ],
        )
        .expect("documented runtime grants must permit append-only audit writes");
    let visible: i64 = client
        .query_one(
            "SELECT count(*)::bigint FROM audit_evidence_record \
             WHERE audit_event_ref = 'audit_event_runtime_grant_01'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(visible, 1);

    for statement in [
        "UPDATE audit_evidence_record SET actor_ref = 'actor_runtime_beta' WHERE audit_event_ref = 'audit_event_runtime_grant_01'",
        "DELETE FROM audit_evidence_record WHERE audit_event_ref = 'audit_event_runtime_grant_01'",
        "TRUNCATE TABLE audit_evidence_record",
    ] {
        let error = client
            .batch_execute(statement)
            .expect_err("runtime audit role must remain append-only after explicit deployment grants");
        assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
    }

    client.batch_execute("RESET ROLE").unwrap();
    client
        .batch_execute(&format!(
            "DROP OWNED BY {runtime_role}; DROP ROLE {runtime_role};"
        ))
        .unwrap();
}

#[test]
fn core_migration_rejects_preexisting_relation_with_wrong_owned_schema() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_shape_test CASCADE; \
             CREATE SCHEMA audit_migration_shape_test; \
             SET search_path TO audit_migration_shape_test; \
             CREATE TABLE audit_evidence_record (\
                 audit_event_ref TEXT NOT NULL, \
                 tenant_ref TEXT NOT NULL \
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
fn core_migration_rejects_same_named_wrong_tenant_time_index() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_index_shape_test CASCADE; \
             CREATE SCHEMA audit_migration_index_shape_test; \
             SET search_path TO audit_migration_index_shape_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();
    client
        .batch_execute(
            "DROP INDEX audit_evidence_tenant_time_index; \
             CREATE INDEX audit_evidence_tenant_time_index \
             ON audit_evidence_record (audit_event_ref);",
        )
        .unwrap();

    let error = apply_audit_evidence_migration(&mut client)
        .expect_err("same-named wrong index must not satisfy tenant-time retention/read evidence");
    let message = error.as_db_error().map_or_else(
        || error.to_string(),
        |database| database.message().to_owned(),
    );
    assert!(
        message.contains("audit_evidence_tenant_time_index") && message.contains("contract"),
        "migration must identify index-definition drift instead of trusting its name: {message}"
    );
}

#[test]
fn both_migrations_apply_inside_the_caller_transaction() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_migration_transaction_test CASCADE; \
             CREATE SCHEMA audit_migration_transaction_test; \
             SET search_path TO audit_migration_transaction_test;",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    apply_audit_evidence_migration(&mut transaction).unwrap();
    apply_audit_evidence_retention_migration(&mut transaction).unwrap();
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT \
                 (SELECT count(*)::bigint FROM information_schema.tables \
                  WHERE table_schema = 'audit_migration_transaction_test' \
                    AND table_name = 'audit_evidence_record'), \
                 (SELECT count(*)::bigint FROM information_schema.routines \
                  WHERE routine_schema = 'audit_migration_transaction_test' \
                    AND routine_name = 'expire_audit_evidence_before'), \
                 (SELECT prosecdef FROM pg_proc \
                  WHERE oid = 'audit_migration_transaction_test.expire_audit_evidence_before(text,bigint)'::regprocedure), \
                 (SELECT pg_get_userbyid(proowner) FROM pg_proc \
                  WHERE oid = 'audit_migration_transaction_test.expire_audit_evidence_before(text,bigint)'::regprocedure)",
            &[],
        )
        .unwrap();
    let table_count: i64 = row.get(0);
    let routine_count: i64 = row.get(1);
    let security_definer: bool = row.get(2);
    let routine_owner: String = row.get(3);
    assert_eq!(table_count, 1);
    assert_eq!(routine_count, 1);
    assert!(
        security_definer,
        "bounded retention must remain SECURITY DEFINER"
    );
    assert_eq!(routine_owner, AUDIT_EVIDENCE_OWNER_ROLE);
}

#[test]
fn retention_migration_fails_closed_when_core_audit_table_is_absent() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_retention_without_core_test CASCADE; \
             CREATE SCHEMA audit_retention_without_core_test; \
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
