//! Read-only response-event reconstruction must remain usable inside stronger transaction snapshots.
//!
//! Response-event writes require `READ COMMITTED` so a concurrent unique-key winner can be
//! observed by replay classification. Pure reconstruction does not perform that race-sensitive
//! write/classify sequence and therefore must not reject a caller that already owns a
//! `REPEATABLE READ` transaction.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, load_response_event_receipts, load_response_ledger,
};

#[test]
fn read_only_reload_accepts_repeatable_read_snapshot() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS response_event_read_isolation_contract CASCADE; \
             CREATE SCHEMA response_event_read_isolation_contract; \
             SET search_path TO response_event_read_isolation_contract;",
        )
        .unwrap();
    apply_response_event_migration(&mut client).unwrap();
    client
        .execute(
            "INSERT INTO response_event (\
                 response_event_ref, session_ref, client_event_ref, item_version_ref, \
                 payload_digest, server_sequence, observed_at, received_at\
             ) VALUES (\
                 'server_event_isolation_01', 'session_isolation_01', 'client_event_isolation_01', \
                 'item_version_isolation_01', \
                 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
                 1, TIMESTAMPTZ '2026-08-28 08:00:00+00', \
                 TIMESTAMPTZ '2026-08-28 08:00:00.250+00'\
             )",
            &[],
        )
        .unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();

    let receipts = load_response_event_receipts(&mut transaction, "session_isolation_01")
        .expect("read-only receipt reconstruction must accept a repeatable-read snapshot");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].event().server_event_ref(), "server_event_isolation_01");

    let ledger = load_response_ledger(&mut transaction, "session_isolation_01")
        .expect("read-only ledger reconstruction must accept a repeatable-read snapshot");
    assert!(!ledger.is_empty());

    transaction.rollback().unwrap();
}
