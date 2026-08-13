//! Regression for schema-scoped outbox lease migration constraints.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable")
}

fn constraint_count(client: &mut Client, schema_name: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)
             FROM pg_constraint AS constraint_row
             JOIN pg_class AS relation_row
               ON relation_row.oid = constraint_row.conrelid
             JOIN pg_namespace AS namespace_row
               ON namespace_row.oid = relation_row.relnamespace
             WHERE constraint_row.conname = 'integration_outbox_lease_presence_check'
               AND relation_row.relname = 'integration_outbox'
               AND namespace_row.nspname = $1",
            &[&schema_name],
        )
        .unwrap()
        .get(0)
}

#[test]
fn lease_presence_constraint_is_installed_independently_in_each_schema() {
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
        constraint_count(&mut client, "outbox_lease_migration_alpha"),
        1,
        "the first isolated schema must own its lease presence constraint"
    );
    assert_eq!(
        constraint_count(&mut client, "outbox_lease_migration_beta"),
        1,
        "a same-named constraint in another schema must not suppress this schema's constraint"
    );
}
