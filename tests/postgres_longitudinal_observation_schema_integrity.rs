//! Real `PostgreSQL` integrity contracts for longitudinal observation evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_longitudinal_observation::apply_longitudinal_observation_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client(schema_name: &str) -> Client {
    assert!(
        schema_name
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_'),
        "schema names must be two-word snake_case identifiers"
    );
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema_name} CASCADE; \
             CREATE SCHEMA {schema_name}; \
             SET search_path TO {schema_name};"
        ))
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
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_membership_test");
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
fn digit_bearing_opaque_references_remain_valid_at_the_database_boundary() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_opaque_reference_test");
    apply_longitudinal_observation_migration(&mut client).unwrap();

    for reference in [
        "longitudinal_observation_record_001",
        "gyeot_observation_20260818_001",
        "clinic_ward_seoul_01",
    ] {
        let is_valid: bool = client
            .query_one("SELECT longitudinal_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            is_valid,
            "opaque references containing digits must not be misclassified as numeric-like: {reference:?}"
        );
    }
}

#[test]
fn numeric_like_references_are_rejected_by_the_database_boundary() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_reference_test");
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
fn unicode_numeric_separator_aliases_are_rejected_by_the_database_boundary() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_unicode_reference_test");
    apply_longitudinal_observation_migration(&mut client).unwrap();

    for reference in ["1．5", "1，000", "1٫5", "1٬000"] {
        let is_valid: bool = client
            .query_one("SELECT longitudinal_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            !is_valid,
            "Unicode numeric separator alias must not be accepted: {reference:?}"
        );
    }
}

#[test]
fn embedded_control_characters_are_rejected_by_the_database_boundary() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_control_reference_test");
    apply_longitudinal_observation_migration(&mut client).unwrap();

    for reference in ["tenant_\u{0001}_clinic", "tenant_\u{001f}_clinic"] {
        let is_valid: bool = client
            .query_one("SELECT longitudinal_reference_is_valid($1)", &[&reference])
            .unwrap()
            .get(0);
        assert!(
            !is_valid,
            "control-character reference alias must not be accepted: {reference:?}"
        );
    }
}

#[test]
fn anomaly_relation_remains_a_check_constraint_when_immutability_is_disabled() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_anomaly_update_test");
    apply_longitudinal_observation_migration(&mut client).unwrap();

    client
        .batch_execute(
            "BEGIN; \
             INSERT INTO longitudinal_observation (\
                 observation_record_ref, tenant_ref, enrollment_ref, source_system_ref, \
                 source_observation_ref, construct_ref, measure_ref, validity_start_at_unix_ms, \
                 validity_end_at_unix_ms, recorded_at_unix_ms, received_at_unix_ms, \
                 ingested_at_unix_ms, timezone_name, utc_offset_minutes, clock_anomaly_code\
             ) VALUES (\
                 'longitudinal_observation_anomaly_update', 'tenant_clinic_seoul', \
                 'longitudinal_enrollment_anomaly_update', 'gyeot_mobile_collection', \
                 'gyeot_observation_anomaly_update', 'construct_extraversion', \
                 'measure_ipip_extraversion_ko_v1', 1776661900000, 1776662200000, \
                 1776662200000, 1776662260000, 1776662270000, 'Asia/Seoul', 540, NULL\
             ); \
             INSERT INTO longitudinal_membership_share (\
                 observation_record_ref, membership_sequence, membership_context_ref, \
                 weight_parts_per_10_000\
             ) VALUES (\
                 'longitudinal_observation_anomaly_update', 1, 'clinic_ward_seoul_01', 10000\
             ); \
             COMMIT; \
             ALTER TABLE longitudinal_observation \
             DISABLE TRIGGER longitudinal_observation_immutable_update;",
        )
        .unwrap();

    let error = client
        .execute(
            "UPDATE longitudinal_observation \
             SET clock_anomaly_code = 'recorded_after_received' \
             WHERE observation_record_ref = 'longitudinal_observation_anomaly_update'",
            &[],
        )
        .expect_err("the CHECK constraint must reject a code without a clock inversion");
    assert_check_constraint(&error, "longitudinal_observation_anomaly_check");
}

#[test]
fn anomaly_code_must_match_the_observed_clock_order() {
    let _guard = guard();
    let mut client = client("longitudinal_observation_schema_integrity_anomaly_test");
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
