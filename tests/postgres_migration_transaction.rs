//! PostgreSQL migration-chain acceptance for transactional rollback safety.

use postgres::{Client, NoTls};
use std::fs;
use std::path::{Path, PathBuf};

const TEST_SCHEMA: &str = "migration_transaction_test";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn migration_files() -> Vec<PathBuf> {
    let migration_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<PathBuf> = fs::read_dir(migration_directory)
        .expect("repository migrations directory must be readable")
        .map(|entry| entry.expect("migration directory entry must be readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "repository must contain PostgreSQL migrations");
    files
}

fn migration_sql(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("migration {} must be readable as UTF-8: {error}", path.display())
    })
}

#[test]
fn complete_migration_chain_is_transactional_and_rolls_back_cleanly() {
    let mut client = test_client();
    assert!(
        !client
            .query_one("SELECT to_regnamespace($1) IS NOT NULL", &[&TEST_SCHEMA])
            .unwrap()
            .get::<_, bool>(0),
        "isolated CI database must not contain the migration test schema before execution"
    );

    let files = migration_files();
    let mut transaction = client.transaction().unwrap();
    transaction
        .batch_execute(&format!(
            "CREATE SCHEMA {TEST_SCHEMA}; SET LOCAL search_path TO {TEST_SCHEMA}"
        ))
        .unwrap();

    for path in &files {
        transaction
            .batch_execute(&migration_sql(path))
            .unwrap_or_else(|error| panic!("migration {} must apply: {error}", path.display()));
    }

    let table_count: i64 = transaction
        .query_one(
            "SELECT count(*)::bigint FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(table_count > 0, "migration chain must create product tables");

    transaction.rollback().unwrap();

    let schema_exists: bool = client
        .query_one("SELECT to_regnamespace($1) IS NOT NULL", &[&TEST_SCHEMA])
        .unwrap()
        .get(0);
    assert!(
        !schema_exists,
        "rolling back the migration transaction must leave no partial schema"
    );
}
