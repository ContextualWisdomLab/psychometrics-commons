//! Real `PostgreSQL` contract for canonical response-payload digest evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response::apply_response_event_migration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn constraint_name(error: &postgres::Error) -> &str {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
}

#[test]
fn persisted_payload_digest_requires_canonical_lowercase_sha256() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS response_digest_constraint_test CASCADE;\
             CREATE SCHEMA response_digest_constraint_test;\
             SET search_path TO response_digest_constraint_test;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
        .execute(
            "INSERT INTO response_event_ledger (session_ref) VALUES ('session_digest_contract')",
            &[],
        )
        .unwrap();

    for (index, invalid_digest) in [
        "sha256:placeholder",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef",
        " sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    ]
    .into_iter()
    .enumerate()
    {
        let server_event_ref = format!("server_event_digest_{index}");
        let client_event_ref = format!("client_event_digest_{index}");
        let sequence = i64::try_from(index + 1).unwrap();
        let error = client
            .execute(
                "INSERT INTO response_event (\
                     session_ref, server_event_ref, client_event_ref, item_version_ref, \
                     payload_digest, server_sequence\
                 ) VALUES ('session_digest_contract', $1, $2, 'item_version_digest', $3, $4)",
                &[
                    &server_event_ref,
                    &client_event_ref,
                    &invalid_digest,
                    &sequence,
                ],
            )
            .unwrap_err();
        assert_eq!(
            constraint_name(&error),
            "response_event_payload_digest_format_check",
            "unexpected constraint for digest fixture {index}"
        );
    }

    client
        .batch_execute("DROP SCHEMA response_digest_constraint_test CASCADE;")
        .unwrap();
}
