//! Least-privilege migration authority regression for the dedicated audit owner.
//!
//! Runtime and maintenance identities must never be able to assume the NOLOGIN role that owns the
//! append-only table and SECURITY DEFINER retention boundary. The hardening migration detects any
//! non-superuser membership path and fails closed before transferring ownership.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

#[test]
fn migration_rejects_any_non_superuser_membership_in_the_dedicated_owner() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_owner_authority_test CASCADE;\
             CREATE SCHEMA audit_owner_authority_test;\
             SET search_path TO audit_owner_authority_test;",
        )
        .expect("isolated audit-owner authority schema must be created");
    apply_audit_evidence_migration(&mut client)
        .expect("baseline migration must establish the dedicated non-login owner");

    let backend_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .expect("backend identity must be available for an isolated probe role")
        .get(0);
    let probe_role = format!("audit_owner_probe_{backend_pid}");

    let mut transaction = client
        .transaction()
        .expect("owner-membership probe transaction must start");
    transaction
        .batch_execute(&format!(
            "CREATE ROLE {probe_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;\
             GRANT psychometrics_audit_evidence_owner TO {probe_role};"
        ))
        .expect("superuser fixture must be able to create an unsafe membership path transactionally");

    let error = apply_audit_evidence_migration(&mut transaction)
        .expect_err("migration must fail closed while a non-superuser can assume the audit owner");
    assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
    assert!(
        error
            .as_db_error()
            .is_some_and(|database| database.message().contains(&probe_role)),
        "failure evidence must identify the non-superuser role that makes the owner assumable"
    );

    transaction
        .rollback()
        .expect("rollback must remove the transactional probe role and membership");
}

#[test]
fn owner_hardening_requires_a_superuser_migration_executor() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let backend_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .expect("backend identity must be available for an isolated migrator role")
        .get(0);
    let migrator_role = format!("audit_migrator_probe_{backend_pid}");
    let schema_name = format!("audit_owner_migrator_{backend_pid}");

    let mut transaction = client
        .transaction()
        .expect("non-superuser migration probe transaction must start");
    transaction
        .batch_execute(&format!(
            "CREATE ROLE {migrator_role} NOLOGIN NOSUPERUSER NOCREATEDB CREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;\
             CREATE SCHEMA {schema_name} AUTHORIZATION {migrator_role};\
             SET ROLE {migrator_role};\
             SET search_path TO {schema_name};"
        ))
        .expect("superuser fixture must establish the isolated CREATEROLE migration probe");

    let error = apply_audit_evidence_migration(&mut transaction)
        .expect_err("audit owner hardening must reject a non-superuser migration executor");
    assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
    assert_eq!(
        error.as_db_error().map(postgres::error::DbError::message),
        Some("audit evidence owner hardening requires a superuser migration executor")
    );

    transaction
        .rollback()
        .expect("rollback must remove the isolated non-superuser migration probe");
}
