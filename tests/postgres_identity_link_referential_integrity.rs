//! Real `PostgreSQL` referential-integrity contract for identity-link end evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_identity_link::apply_identity_link_migration;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[test]
fn link_end_event_requires_an_existing_link_event_in_the_same_participant_scope() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS identity_link_referential_integrity_test CASCADE;\
             CREATE SCHEMA identity_link_referential_integrity_test;\
             SET search_path TO identity_link_referential_integrity_test;",
        )
        .unwrap();
    apply_identity_link_migration(&mut client).unwrap();

    client
        .execute(
            "INSERT INTO participant_identity_ledger (\
                 participant_ref, tenant_ref, created_at_unix_ms\
             ) VALUES ('participant_orphan_end', 'tenant_alpha', 10000)",
            &[],
        )
        .unwrap();

    let orphan_end = client.execute(
        "INSERT INTO participant_identity_link_end_event (\
             participant_ref, link_end_event_ref, linked_event_ref, evidence_ref, ended_at_unix_ms\
         ) VALUES (\
             'participant_orphan_end', 'link_end_orphan', 'link_event_missing',\
             'evidence_unlink_orphan', 11000\
         )",
        &[],
    );
    assert!(
        orphan_end.is_err(),
        "PostgreSQL accepted link-end evidence that does not reference an existing link event"
    );

    client
        .batch_execute("DROP SCHEMA identity_link_referential_integrity_test CASCADE;")
        .unwrap();
}
