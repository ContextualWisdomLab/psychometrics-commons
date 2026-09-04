//! Corrupt durable response-event evidence must be classified separately from replay conflicts.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_ledger, ResponseEventPersistenceError,
};

#[test]
fn noncanonical_stored_digest_is_corrupt_evidence_not_replay_conflict() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS response_event_corrupt_evidence_test;\
             SET search_path TO response_event_corrupt_evidence_test;\
             DROP TABLE IF EXISTS response_event_corrupt_evidence_test.response_event;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE response_event DROP CONSTRAINT response_event_payload_digest_format_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_corrupt_01', 'session_ipip_ko_corrupt', 'client_event_corrupt_01', \
                 'item_version_n1_ko', 'not-a-digest', 1, \
                 TIMESTAMPTZ '2023-11-14 22:13:20+00', \
                 TIMESTAMPTZ '2023-11-14 22:13:20.250+00'\
             )",
            &[],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = load_response_ledger(&mut transaction, "session_ipip_ko_corrupt").unwrap_err();
    assert!(matches!(
        error,
        ResponseEventPersistenceError::CorruptStoredEvidence
    ));
    transaction.rollback().unwrap();
}
