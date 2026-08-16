//! Real `PostgreSQL` bounds for durable assessment-participant rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_participant::apply_assessment_participant_migration;
use std::sync::{Mutex, MutexGuard};

static PARTICIPANT_SCHEMA_LOCK: Mutex<()> = Mutex::new(());

fn schema_test_guard() -> MutexGuard<'static, ()> {
    PARTICIPANT_SCHEMA_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS assessment_participant_schema_test;\
             SET search_path TO assessment_participant_schema_test;",
        )
        .unwrap();
    client
}

fn reset_schema(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS assessment_participant_schema_test.assessment_participant;",
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

#[test]
fn schema_rejects_numeric_identity_zero_time_invalid_status_and_mutation() {
    let _guard = schema_test_guard();
    let mut client = test_client();
    reset_schema(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();
    apply_assessment_participant_migration(&mut client).unwrap();

    let numeric = client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('12', 'tenant_alpha', 'anonymous', 1000)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric),
        "assessment_participant_participant_ref_format_check"
    );

    let padded_tenant = client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_schema_alpha', ' tenant_alpha', 'anonymous', 1000)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&padded_tenant),
        "assessment_participant_tenant_ref_format_check"
    );

    let zero_time = client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_schema_alpha', 'tenant_alpha', 'anonymous', 0)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&zero_time),
        "assessment_participant_created_at_unix_positive_check"
    );

    let unknown_status = client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_schema_alpha', 'tenant_alpha', 'linked', 1000)",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&unknown_status),
        "assessment_participant_status_value_check"
    );

    client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_schema_alpha', 'tenant_alpha', 'anonymous', 1000)",
            &[],
        )
        .unwrap();

    let update = client
        .execute(
            "UPDATE assessment_participant SET created_at_unix_ms = 2000 \
             WHERE participant_ref = 'participant_schema_alpha'",
            &[],
        )
        .unwrap_err();
    assert_eq!(
        update
            .as_db_error()
            .expect("immutable participant evidence must fail at the database boundary")
            .code()
            .code(),
        "55000"
    );
}
