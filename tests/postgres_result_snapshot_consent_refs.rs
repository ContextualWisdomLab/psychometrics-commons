//! Real `PostgreSQL` integrity contracts for result consent-snapshot references.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::sync::{Mutex, MutexGuard};

static RESULT_CONSENT_SCHEMA_LOCK: Mutex<()> = Mutex::new(());

fn schema_test_guard() -> MutexGuard<'static, ()> {
    RESULT_CONSENT_SCHEMA_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS result_snapshot_consent_ref_test;\
             SET search_path TO result_snapshot_consent_ref_test;",
        )
        .unwrap();
    client
}

fn reset_schema(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS result_snapshot_consent_ref_test.result_snapshot_observation;\
             DROP TABLE IF EXISTS result_snapshot_consent_ref_test.result_snapshot;",
        )
        .unwrap();
}

fn constraint_name(error: &postgres::Error) -> String {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
        .to_owned()
}

const SNAPSHOT_COLUMNS: &str =
    "result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
     response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
     scoring_version_ref, calibration_reference, norm_version_ref, \
     requested_output_schema_version, narrative_version_ref, \
     consent_snapshot_refs, engine_artifact_digest, created_at_unix_ms";

const VALID_SNAPSHOT_VALUES: &str = "'result_snapshot_consent_valid', 'participant_result_one', \
     'scoring_result_result_one', 'session_result_one', \
     'response_snapshot_result_one', 'assessment_spec_big_five_v1', \
     'instrument_version_big_five_ko_v1', 'scoring_version_big_five_v1', \
     'calibration_big_five_ko_v1', NULL, 1, 'narrative_version_big_five_v1', \
     ARRAY['consent_snapshot_service_v1'], \
     'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 70000";

fn invalid_consent_insert(client: &mut Client, array_sql: &str) -> postgres::Error {
    client
        .execute(
            &format!(
                "INSERT INTO result_snapshot ({SNAPSHOT_COLUMNS}) VALUES ({})",
                VALID_SNAPSHOT_VALUES.replace("ARRAY['consent_snapshot_service_v1']", array_sql,)
            ),
            &[],
        )
        .unwrap_err()
}

#[test]
fn schema_rejects_null_numeric_blank_control_and_duplicate_consent_references() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    for invalid_array in [
        "ARRAY[NULL]::TEXT[]",
        "ARRAY['12']",
        "ARRAY[' ']",
        "ARRAY[E'\\t']",
        "ARRAY[E'consent_snapshot_service_v1\\n']",
        "ARRAY['consent_snapshot_service_v1', 'consent_snapshot_service_v1']",
    ] {
        let error = invalid_consent_insert(&mut client, invalid_array);
        assert_eq!(
            constraint_name(&error),
            "result_snapshot_consent_refs_integrity_check"
        );
    }
}

#[test]
fn migration_reapplication_repairs_a_weakened_consent_reference_constraint() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_result_snapshot_migration(&mut client).unwrap();

    client
        .batch_execute(
            "ALTER TABLE result_snapshot \
                 DROP CONSTRAINT IF EXISTS result_snapshot_consent_refs_integrity_check;\
             ALTER TABLE result_snapshot \
                 ADD CONSTRAINT result_snapshot_consent_refs_integrity_check \
                 CHECK (cardinality(consent_snapshot_refs) > 0);",
        )
        .unwrap();

    apply_result_snapshot_migration(&mut client).unwrap();

    let duplicate = invalid_consent_insert(
        &mut client,
        "ARRAY['consent_snapshot_service_v1', 'consent_snapshot_service_v1']",
    );
    assert_eq!(
        constraint_name(&duplicate),
        "result_snapshot_consent_refs_integrity_check"
    );
}
