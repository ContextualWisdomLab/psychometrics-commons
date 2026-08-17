//! Executable `PostgreSQL` prerequisites for product-owned persistence migrations.
//!
//! `migrations/0030_assessment_participant.sql` uses the `PostgreSQL` 18
//! `pg_unicode_fast` collation. The runtime contract therefore fails closed when
//! CI or a deployment points the persistence suite at another `PostgreSQL` major
//! or at a non-UTF-8 database.

use postgres::{Client, NoTls};

#[test]
fn persistence_database_is_postgresql_18_utf8() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");

    let version: String = client
        .query_one("SHOW server_version_num", &[])
        .expect("PostgreSQL must expose server_version_num")
        .get(0);
    let version: u32 = version
        .parse()
        .expect("server_version_num must be a decimal integer");
    assert!(
        (180_000..190_000).contains(&version),
        "product persistence requires PostgreSQL major version 18; got server_version_num={version}"
    );

    let encoding: String = client
        .query_one("SHOW server_encoding", &[])
        .expect("PostgreSQL must expose server_encoding")
        .get(0);
    assert_eq!(
        encoding, "UTF8",
        "product persistence requires a UTF-8 PostgreSQL database"
    );
}
