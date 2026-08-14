//! Database-level immutability coverage for research authorization, contribution, and withdrawal evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_research_contribution::apply_research_contribution_migration;
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
    let schema_name = format!("research_contribution_immutable_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_research_contribution_migration(&mut client).unwrap();
    client
        .batch_execute(
            "INSERT INTO research_consent_snapshot (\
                 consent_snapshot_ref, participant_ref, research_scope_ref, consent_form_version_ref\
             ) VALUES (\
                 'consent_snapshot_immutable', 'participant_immutable',\
                 'research_scope_immutable', 'consent_form_immutable'\
             );\
             INSERT INTO research_contribution (\
                 contribution_ref, participant_ref, research_participant_ref,\
                 consent_snapshot_ref, research_scope_ref, started_at_unix_ms\
             ) VALUES (\
                 'research_contribution_immutable', 'participant_immutable',\
                 'research_participant_immutable', 'consent_snapshot_immutable',\
                 'research_scope_immutable', 10000\
             );\
             INSERT INTO research_withdrawal_event (\
                 contribution_ref, withdrawal_event_ref, withdrawn_at_unix_ms\
             ) VALUES (\
                 'research_contribution_immutable', 'withdrawal_event_immutable', 11000\
             );",
        )
        .unwrap();
    (client, schema_name)
}

fn assert_immutable_error(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("immutable research evidence must fail at the database boundary");
    assert_eq!(database_error.code().code(), "55000");
}

fn expect_rejected_statement(client: &mut Client, statement: &str) {
    let mut transaction = client.transaction().unwrap();
    let error = transaction
        .batch_execute(statement)
        .expect_err("immutable research evidence mutation must be rejected");
    assert_immutable_error(&error);
    transaction.rollback().unwrap();
}

#[test]
fn research_evidence_rejects_update_delete_and_truncate() {
    let (mut client, schema_name) = isolated_client();

    let mutations = [
        "UPDATE research_consent_snapshot SET research_scope_ref = 'research_scope_tampered' \
         WHERE consent_snapshot_ref = 'consent_snapshot_immutable'",
        "UPDATE research_contribution SET research_participant_ref = 'research_participant_tampered' \
         WHERE contribution_ref = 'research_contribution_immutable'",
        "UPDATE research_withdrawal_event SET withdrawn_at_unix_ms = 12000 \
         WHERE contribution_ref = 'research_contribution_immutable'",
        "DELETE FROM research_withdrawal_event \
         WHERE contribution_ref = 'research_contribution_immutable'",
        "DELETE FROM research_contribution \
         WHERE contribution_ref = 'research_contribution_immutable'",
        "DELETE FROM research_consent_snapshot \
         WHERE consent_snapshot_ref = 'consent_snapshot_immutable'",
        "TRUNCATE TABLE research_withdrawal_event",
        "TRUNCATE TABLE research_contribution CASCADE",
        "TRUNCATE TABLE research_consent_snapshot CASCADE",
    ];
    for statement in mutations {
        expect_rejected_statement(&mut client, statement);
    }

    let counts = client
        .query_one(
            "SELECT\
                 (SELECT count(*) FROM research_consent_snapshot),\
                 (SELECT count(*) FROM research_contribution),\
                 (SELECT count(*) FROM research_withdrawal_event)",
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
