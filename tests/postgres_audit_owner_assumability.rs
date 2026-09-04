//! Fail-closed deployment contracts for the dedicated audit-evidence owner role.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

const AUDIT_EVIDENCE_OWNER_ROLE: &str = "psychometrics_audit_evidence_owner";

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn owner_hardening_rejects_any_non_superuser_role_that_can_assume_the_owner() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_owner_assumability_test CASCADE; \
             CREATE SCHEMA audit_owner_assumability_test; \
             SET search_path TO audit_owner_assumability_test;",
        )
        .unwrap();
    apply_audit_evidence_migration(&mut client).unwrap();

    let backend_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    let unsafe_role = format!("audit_owner_assumer_{}_{}", std::process::id(), backend_pid);

    let error = {
        let mut transaction = client.transaction().unwrap();
        transaction
            .batch_execute(&format!(
                "CREATE ROLE {unsafe_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS; \
                 GRANT {AUDIT_EVIDENCE_OWNER_ROLE} TO {unsafe_role}; \
                 SET LOCAL search_path TO audit_owner_assumability_test;"
            ))
            .unwrap();

        apply_audit_evidence_migration(&mut transaction)
            .expect_err("owner hardening must fail closed while a non-superuser can assume the dedicated audit owner")
    };

    assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
    let message = error.as_db_error().map_or_else(
        || error.to_string(),
        |database| database.message().to_owned(),
    );
    assert!(
        message.contains("dedicated audit evidence owner role is assumable")
            && message.contains(&unsafe_role),
        "migration must name the unsafe owner-membership boundary: {message}"
    );

    let role_persisted: bool = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)",
            &[&unsafe_role],
        )
        .unwrap()
        .get(0);
    assert!(
        !role_persisted,
        "the failed migration probe must roll back its temporary unsafe role"
    );
}
