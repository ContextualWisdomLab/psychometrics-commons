//! `PostgreSQL` migration-chain acceptance for transactional rollback safety.

use postgres::{Client, NoTls, Transaction};
use std::fs;
use std::path::{Path, PathBuf};

const ROLLBACK_SCHEMA: &str = "migration_transaction_rollback_test";
const REAPPLY_SCHEMA: &str = "migration_transaction_reapply_test";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn migration_files() -> Vec<PathBuf> {
    let migration_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<PathBuf> = fs::read_dir(migration_directory)
        .expect("repository migrations directory must be readable")
        .map(|entry| {
            entry
                .expect("migration directory entry must be readable")
                .path()
        })
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "repository must contain PostgreSQL migrations"
    );
    files
}

fn migration_sql(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "migration {} must be readable as UTF-8: {error}",
            path.display()
        )
    })
}

fn schema_exists(client: &mut Client, schema: &str) -> bool {
    client
        .query_one("SELECT to_regnamespace($1) IS NOT NULL", &[&schema])
        .unwrap()
        .get(0)
}

fn apply_migration_chain(transaction: &mut Transaction<'_>, files: &[PathBuf]) {
    for path in files {
        transaction
            .batch_execute(&migration_sql(path))
            .unwrap_or_else(|error| panic!("migration {} must apply: {error}", path.display()));
    }
}

fn product_table_count(transaction: &mut Transaction<'_>) -> i64 {
    transaction
        .query_one(
            "SELECT count(*)::bigint FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
            &[],
        )
        .unwrap()
        .get(0)
}

#[test]
fn complete_migration_chain_is_transactional_and_rolls_back_cleanly() {
    let mut client = test_client();
    assert!(
        !schema_exists(&mut client, ROLLBACK_SCHEMA),
        "isolated CI database must not contain the rollback-test schema before execution"
    );

    let files = migration_files();
    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute(&format!(
            "CREATE SCHEMA {ROLLBACK_SCHEMA}; SET LOCAL search_path TO {ROLLBACK_SCHEMA}"
        ))
        .unwrap();
    apply_migration_chain(&mut transaction, &files);
    assert!(
        product_table_count(&mut transaction) > 0,
        "migration chain must create product tables"
    );

    transaction.rollback().unwrap();
    assert!(
        !schema_exists(&mut client, ROLLBACK_SCHEMA),
        "rolling back the migration transaction must leave no partial schema"
    );
}

#[test]
fn complete_migration_chain_reapplies_without_schema_drift() {
    let mut client = test_client();
    assert!(
        !schema_exists(&mut client, REAPPLY_SCHEMA),
        "isolated CI database must not contain the reapply-test schema before execution"
    );

    let files = migration_files();
    let initial_table_count = {
        let mut transaction = client.transaction().unwrap();
        transaction
            .batch_execute(&format!(
                "CREATE SCHEMA {REAPPLY_SCHEMA}; SET LOCAL search_path TO {REAPPLY_SCHEMA}"
            ))
            .unwrap();
        apply_migration_chain(&mut transaction, &files);
        let table_count = product_table_count(&mut transaction);
        transaction.commit().unwrap();
        table_count
    };
    assert!(
        initial_table_count > 0,
        "initial migration chain must create product tables"
    );

    let reapplied_table_count = {
        let mut transaction = client.transaction().unwrap();
        transaction
            .batch_execute(&format!("SET LOCAL search_path TO {REAPPLY_SCHEMA}"))
            .unwrap();
        apply_migration_chain(&mut transaction, &files);
        let table_count = product_table_count(&mut transaction);
        transaction.commit().unwrap();
        table_count
    };
    assert_eq!(
        reapplied_table_count, initial_table_count,
        "reapplying the complete migration chain must preserve the physical table set"
    );

    client
        .batch_execute(&format!("DROP SCHEMA {REAPPLY_SCHEMA} CASCADE"))
        .unwrap();
    assert!(
        !schema_exists(&mut client, REAPPLY_SCHEMA),
        "reapply-test schema cleanup must succeed"
    );
}
