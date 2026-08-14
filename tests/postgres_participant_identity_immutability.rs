//! Database-level immutability coverage for append-only participant identity-link history.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_participant_identity::apply_participant_identity_migration;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    let schema_name = format!("participant_identity_immutable_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_participant_identity_migration(&mut client).unwrap();
    client
        .batch_execute(
            "INSERT INTO participant_identity_ledger (\
                 participant_ref, tenant_ref, created_at_unix_ms\
             ) VALUES ('participant_immutable', 'tenant_immutable', 10000);\
             INSERT INTO participant_identity_link_event (\
                 participant_ref, link_event_ref, issuer_ref, subject_ref,\
                 anonymous_proof_ref, authenticated_proof_ref, linked_at_unix_ms\
             ) VALUES (\
                 'participant_immutable', 'link_event_immutable', 'issuer_immutable',\
                 'subject_immutable', 'anonymous_proof_immutable',\
                 'authenticated_proof_immutable', 11000\
             );\
             INSERT INTO participant_identity_link_end_event (\
                 participant_ref, link_end_event_ref, linked_event_ref, evidence_ref,\
                 ended_at_unix_ms\
             ) VALUES (\
                 'participant_immutable', 'link_end_event_immutable', 'link_event_immutable',\
                 'unlink_evidence_immutable', 12000\
             );",
        )
        .unwrap();
    (client, schema_name)
}

fn assert_immutable_error(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("append-only identity evidence must fail at the database boundary");
    assert_eq!(database_error.code().code(), "55000");
}

fn expect_rejected_statement(client: &mut Client, statement: &str) {
    let mut transaction = client.transaction().unwrap();
    let error = transaction
        .batch_execute(statement)
        .expect_err("append-only identity evidence mutation must be rejected");
    assert_immutable_error(&error);
    transaction.rollback().unwrap();
}

#[test]
fn identity_history_rejects_update_delete_and_truncate() {
    let (mut client, schema_name) = isolated_client();

    let mutations = [
        "UPDATE participant_identity_ledger SET tenant_ref = 'tenant_tampered' \
         WHERE participant_ref = 'participant_immutable'",
        "UPDATE participant_identity_link_event SET subject_ref = 'subject_tampered' \
         WHERE participant_ref = 'participant_immutable'",
        "UPDATE participant_identity_link_end_event SET evidence_ref = 'evidence_tampered' \
         WHERE participant_ref = 'participant_immutable'",
        "DELETE FROM participant_identity_link_end_event \
         WHERE participant_ref = 'participant_immutable'",
        "DELETE FROM participant_identity_link_event \
         WHERE participant_ref = 'participant_immutable'",
        "DELETE FROM participant_identity_ledger \
         WHERE participant_ref = 'participant_immutable'",
        "TRUNCATE TABLE participant_identity_link_end_event",
        "TRUNCATE TABLE participant_identity_link_event CASCADE",
        "TRUNCATE TABLE participant_identity_ledger CASCADE",
    ];
    for statement in mutations {
        expect_rejected_statement(&mut client, statement);
    }

    let counts = client
        .query_one(
            "SELECT\
                 (SELECT count(*) FROM participant_identity_ledger),\
                 (SELECT count(*) FROM participant_identity_link_event),\
                 (SELECT count(*) FROM participant_identity_link_end_event)",
            &[],
        )
        .unwrap();
    assert_eq!(counts.get::<_, i64>(0), 1);
    assert_eq!(counts.get::<_, i64>(1), 1);
    assert_eq!(counts.get::<_, i64>(2), 1);

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}
