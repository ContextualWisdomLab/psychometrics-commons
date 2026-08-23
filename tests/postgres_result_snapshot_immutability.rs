//! Database-level immutability coverage for persisted result snapshots and observations.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::ops::{Deref, DerefMut};

struct SchemaClient {
    client: Client,
    schema_name: String,
}

impl Deref for SchemaClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for SchemaClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Drop for SchemaClient {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "RESET search_path; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema_name
        ));
    }
}

fn isolated_client() -> SchemaClient {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let database_nonce: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for test isolation")
        .get(0);
    let schema_name = format!("result_snapshot_immutable_{database_nonce}");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    let mut client = SchemaClient {
        client,
        schema_name,
    };
    apply_result_snapshot_migration(&mut *client).unwrap();
    client
        .batch_execute(
            "INSERT INTO result_snapshot (\
                 result_snapshot_ref, participant_ref, scoring_result_ref, session_ref,\
                 response_snapshot_ref, assessment_spec_ref, instrument_version_ref,\
                 scoring_version_ref, calibration_reference, norm_version_ref,\
                 requested_output_schema_version, narrative_version_ref, consent_snapshot_refs,\
                 engine_artifact_digest, created_at_unix_ms, supersedes_ref\
             ) VALUES (\
                 'result_snapshot_immutable', 'participant_immutable', 'scoring_result_immutable',\
                 'session_immutable', 'response_snapshot_immutable', 'assessment_spec_immutable',\
                 'instrument_version_immutable', 'scoring_version_immutable',\
                 'calibration_reference_immutable', NULL, 1, 'narrative_version_immutable',\
                 ARRAY['consent_snapshot_immutable'],\
                 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                 10000, NULL\
             );\
             INSERT INTO result_snapshot_observation (\
                 result_snapshot_ref, observation_order, construct_ref,\
                 observation_disposition, score, standard_error\
             ) VALUES (\
                 'result_snapshot_immutable', 0, 'construct_immutable', 'scored', 0.5, 0.1\
             );",
        )
        .unwrap();
    client
}

fn assert_immutable_error(error: &postgres::Error) {
    let database_error = error
        .as_db_error()
        .expect("immutable result evidence must fail at the database boundary");
    assert_eq!(database_error.code().code(), "55000");
}

fn expect_rejected_statement(client: &mut Client, statement: &str) {
    let mut transaction = client.transaction().unwrap();
    let error = transaction
        .batch_execute(statement)
        .expect_err("immutable result evidence mutation must be rejected");
    assert_immutable_error(&error);
    transaction.rollback().unwrap();
}

#[test]
fn result_evidence_rejects_update_delete_and_truncate() {
    let mut client = isolated_client();

    let mutations = [
        "UPDATE result_snapshot SET narrative_version_ref = 'narrative_version_tampered' \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "UPDATE result_snapshot_observation SET score = 0.9 \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "DELETE FROM result_snapshot_observation \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "DELETE FROM result_snapshot \
         WHERE result_snapshot_ref = 'result_snapshot_immutable'",
        "TRUNCATE TABLE result_snapshot_observation",
        "TRUNCATE TABLE result_snapshot CASCADE",
    ];
    for statement in mutations {
        expect_rejected_statement(&mut client, statement);
    }

    let counts = client
        .query_one(
            "SELECT\
                 (SELECT count(*) FROM result_snapshot),\
                 (SELECT count(*) FROM result_snapshot_observation)",
            &[],
        )
        .unwrap();
    assert_eq!(counts.get::<_, i64>(0), 1);
    assert_eq!(counts.get::<_, i64>(1), 1);
}
