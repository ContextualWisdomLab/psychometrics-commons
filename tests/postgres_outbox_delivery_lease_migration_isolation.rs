//! Regression for schema-scoped outbox lease migration constraints.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DATABASE_TEST_LOCK_KEY: i64 = 0x4F55_5442_4F58_4C53;
const LEASE_COLUMN_CONSTRAINTS: [&str; 5] = [
    "integration_outbox_lease_worker_ref_format_check",
    "integration_outbox_lease_ref_format_check",
    "integration_outbox_lease_fencing_token_positive_check",
    "integration_outbox_lease_expiry_positive_check",
    "integration_outbox_delivery_lease_generation_nonnegative_check",
];

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn database_test_guard() -> Client {
    let mut client = test_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL outbox-lease test advisory lock should be acquired");
    client
}

fn constraint_count(client: &mut Client, schema_name: &str, constraint_name: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)
             FROM pg_constraint AS constraint_row
             JOIN pg_class AS relation_row
               ON relation_row.oid = constraint_row.conrelid
             JOIN pg_namespace AS namespace_row
               ON namespace_row.oid = relation_row.relnamespace
             WHERE constraint_row.conname = $2
               AND relation_row.relname = 'integration_outbox'
               AND namespace_row.nspname = $1",
            &[&schema_name, &constraint_name],
        )
        .unwrap()
        .get(0)
}

#[test]
fn lease_presence_constraint_is_installed_independently_in_each_schema() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS outbox_lease_migration_alpha CASCADE;
             DROP SCHEMA IF EXISTS outbox_lease_migration_beta CASCADE;
             CREATE SCHEMA outbox_lease_migration_alpha;
             CREATE SCHEMA outbox_lease_migration_beta;
             SET search_path TO outbox_lease_migration_alpha;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();

    client
        .batch_execute("SET search_path TO outbox_lease_migration_beta;")
        .unwrap();
    apply_integration_migration(&mut client).unwrap();

    assert_eq!(
        constraint_count(
            &mut client,
            "outbox_lease_migration_alpha",
            "integration_outbox_lease_presence_check",
        ),
        1,
        "the first isolated schema must own its lease presence constraint"
    );
    assert_eq!(
        constraint_count(
            &mut client,
            "outbox_lease_migration_beta",
            "integration_outbox_lease_presence_check",
        ),
        1,
        "a same-named constraint in another schema must not suppress this schema's constraint"
    );

    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS outbox_lease_migration_alpha CASCADE;
             DROP SCHEMA IF EXISTS outbox_lease_migration_beta CASCADE;",
        )
        .expect("isolated migration schemas should be removed");
}

#[test]
fn reapplication_repairs_missing_lease_column_constraints() {
    let _database_guard = database_test_guard();
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS outbox_lease_migration_repair CASCADE;
             CREATE SCHEMA outbox_lease_migration_repair;
             SET search_path TO outbox_lease_migration_repair;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();

    for constraint_name in LEASE_COLUMN_CONSTRAINTS {
        client
            .batch_execute(&format!(
                "ALTER TABLE integration_outbox DROP CONSTRAINT IF EXISTS {constraint_name};"
            ))
            .unwrap();
    }

    apply_integration_migration(&mut client).unwrap();

    for constraint_name in LEASE_COLUMN_CONSTRAINTS {
        assert_eq!(
            constraint_count(
                &mut client,
                "outbox_lease_migration_repair",
                constraint_name,
            ),
            1,
            "reapplying migration 0013 must repair missing constraint {constraint_name}"
        );
    }

    client
        .batch_execute("DROP SCHEMA IF EXISTS outbox_lease_migration_repair CASCADE;")
        .expect("isolated migration repair schema should be removed");
}
