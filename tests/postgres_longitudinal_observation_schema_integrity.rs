//! Real `PostgreSQL` integrity contracts for longitudinal observation evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_longitudinal_observation::apply_longitudinal_observation_migration;

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS longitudinal_observation_schema_integrity_test CASCADE; \
             CREATE SCHEMA longitudinal_observation_schema_integrity_test; \
             SET search_path TO longitudinal_observation_schema_integrity_test;",
        )
        .unwrap();
    client
}

fn insert_observation(
    client: &mut Client,
    tenant_ref: &str,
    recorded_at_unix_ms: i64,
    received_at_unix_ms: i64,
    clock_anomaly_code: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO longitudinal_observation (\
             observation_record_ref, tenant_ref, enrollment_ref, source_system_ref, \
             source_observation_ref, construct_ref, measure_ref, validity_start_at_unix_ms, \
             validity_end_at_unix_ms, recorded_at_unix_ms, received_at_unix_ms, \
             ingested_at_unix_ms, timezone_name, utc_offset_minutes, clock_anomaly_code\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        &[
            &"longitudinal_observation_schema_integrity",
            &tenant_ref,
            &"longitudinal_enrollment_schema_integrity",
            &"gyeot_mobile_collection",
            &"gyeot_observation_schema_integrity",
            &"construct_extraversion",
            &"measure_ipip_extraversion_ko_v1",
            &1_776_661_900_000_i64,
            &1_776_662_200_000_i64,
            &recorded_at_unix_ms,
            &received_at_unix_ms,
            &1_776_662_270_000_i64,
            &"Asia/Seoul",
            &540_i16,
            &clock_anomaly_code,
        ],
    )
}

fn assert_check_constraint(error: &postgres::Error, expected_constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("schema integrity rejection must be a PostgreSQL database error");
    assert_eq!(
        database_error.code(),
        &postgres::error::SqlState::CHECK_VIOLATION
    );
    assert_eq!(database_error.constraint(), Some(expected_constraint));
}

#[test]
fn observation_header_cannot_commit_without_complete_membership_vector() {
    let mut client = client();
    apply_longitudinal_observation_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO longitudinal_observation (\
                 observation_record_ref, tenant_ref, enrollment_ref, source_system_ref, \
                 source_observation_ref, construct_ref, measure_ref, validity_start_at_unix_ms, \
                 validity_end_at_unix_ms, recorded_at_unix_ms, received_at_unix_ms, \
                 ingested_at_unix_ms, timezone_name, utc_offset_minutes, clock_anomaly_code\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
            &[
                &"longitudinal_observation_missing_membership",
                &"tenant_clinic_seoul",
                &"longitudinal_enrollment_missing_membership",
                &"gyeot_mobile_collection",
                &"gyeot_observation_missing_membership",
                &"construct_extraversion",
                &"measure_ipip_extraversion_ko_v1",
                &1_776_661_900_000_i64,
                &1_776_662_200_000_i64,
                &1_776_662_200_000_i64,
                &1_776_662_260_000_i64,
                &1_776_662_270_000_i64,
                &"Asia/Seoul",
                &540_i16,
                &Option::<&str>::None,
            ],
        )
        .unwrap();

    let error = transaction
        .commit()
        .expect_err("an observation with no membership rows must fail at commit");
    assert_eq!(
        error.as_db_error().map(postgres::error::DbError::code),
        Some(&postgres::error::SqlState::CHECK_VIOLATION)
    );
}

#[test]
fn numeric_like_references_are_rejected_by_the_database_boundary() {
    let mut client = client();
    apply_longitudinal_observation_migration(&mut client).unwrap();

    let error = insert_observation(
        &mut client,
        "1.5",
        1_776_662_200_000,
        1_776_662_260_000,
        None,
    )
    .expect_err("numeric-like opaque references must fail before evidence is stored");
    assert_check_constraint(&error, "longitudinal_observation_reference_check");
}

#[test]
fn anomaly_code_must_match_the_observed_clock_order() {
    let mut client = client();
    apply_longitudinal_observation_migration(&mut client).unwrap();

    let error = insert_observation(
        &mut client,
        "tenant_clinic_seoul",
        1_776_662_200_000,
        1_776_662_260_000,
        Some("recorded_after_received"),
    )
    .expect_err("an anomaly code without the corresponding clock inversion must fail");
    assert_check_constraint(&error, "longitudinal_observation_anomaly_check");
}
