//! Exhaustive two-way whitespace parity for participant reference constraints.
//!
//! The Rust reference contract rejects leading/trailing `char::is_whitespace`
//! characters through `str::trim`. PostgreSQL 18's `pg_unicode_fast` POSIX
//! `[[:space:]]` class must therefore classify exactly the same representable
//! Unicode scalar set before the migration can claim parity.

use postgres::{Client, NoTls};

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn postgres_unicode_space_class_matches_rust_trim_whitespace_set() {
    let mut client = test_client();

    let postgres_whitespace: Vec<i32> = client
        .query_one(
            "WITH unicode_scalar(codepoint) AS (\
                 SELECT generate_series(1, 55295)::integer\
                 UNION ALL\
                 SELECT generate_series(57344, 1114111)::integer\
             )\
             SELECT COALESCE(array_agg(codepoint ORDER BY codepoint), ARRAY[]::integer[])\
             FROM unicode_scalar\
             WHERE chr(codepoint) COLLATE \"pg_unicode_fast\" ~ '^[[:space:]]$'",
            &[],
        )
        .expect("PostgreSQL whitespace classification query must execute")
        .get(0);

    let rust_whitespace: Vec<i32> = (1u32..=0x0010_FFFF)
        .filter_map(|codepoint| char::from_u32(codepoint).map(|scalar| (codepoint, scalar)))
        .filter(|(_, scalar)| scalar.is_whitespace())
        .map(|(codepoint, _)| i32::try_from(codepoint).expect("Unicode scalar fits i32"))
        .collect();

    assert_eq!(
        postgres_whitespace, rust_whitespace,
        "PostgreSQL pg_unicode_fast [[:space:]] must exactly match Rust char::is_whitespace"
    );
}
