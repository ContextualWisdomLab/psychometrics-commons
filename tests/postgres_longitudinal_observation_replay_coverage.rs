//! Exhaustive replay-boundary coverage for durable longitudinal observations.
//!
//! These tests exercise immutable-evidence mismatches through the public
//! persistence API instead of weakening the production immutability contract.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::longitudinal_observation::{
    LongitudinalObservationInput, LongitudinalObservationRecord, LongitudinalObservationSet,
    MembershipShareInput, ObservationTimeInput,
};
use psychometrics_commons_runtime::postgres_longitudinal_observation::{
    apply_longitudinal_observation_migration, load_longitudinal_observation,
    persist_longitudinal_observation, LongitudinalObservationPersistenceDisposition,
    LongitudinalObservationPersistenceError,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

const BASE_RECORD_REF: &str = "longitudinal_observation_record_coverage";
const BASE_ENROLLMENT_REF: &str = "longitudinal_enrollment_coverage";
const BASE_SOURCE_SYSTEM_REF: &str = "gyeot_mobile_collection";
const BASE_SOURCE_OBSERVATION_REF: &str = "gyeot_observation_coverage";
const BASE_CONSTRUCT_REF: &str = "construct_extraversion";
const BASE_MEASURE_REF: &str = "measure_ipip_extraversion_ko_v1";
const BASE_VALIDITY_START: u64 = 1_776_661_900_000;
const BASE_VALIDITY_END: u64 = 1_776_662_200_000;
const BASE_RECORDED_AT: u64 = 1_776_662_200_000;
const BASE_RECEIVED_AT: u64 = 1_776_662_260_000;
const BASE_INGESTED_AT: u64 = 1_776_662_270_000;
const BASE_TIMEZONE: &str = "Asia/Seoul";
const BASE_OFFSET: i16 = 540;

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS longitudinal_observation_replay_coverage_test CASCADE; \
             CREATE SCHEMA longitudinal_observation_replay_coverage_test; \
             SET search_path TO longitudinal_observation_replay_coverage_test;",
        )
        .unwrap();
    apply_longitudinal_observation_migration(&mut client).unwrap();
    client
}

#[allow(clippy::too_many_arguments)]
fn record(
    observation_record_ref: &str,
    enrollment_ref: &str,
    source_system_ref: &str,
    source_observation_ref: &str,
    construct_ref: &str,
    measure_ref: &str,
    memberships: &[MembershipShareInput<'_>],
    validity_start_at_unix_ms: u64,
    validity_end_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
    timezone_name: &str,
    utc_offset_minutes: i16,
) -> LongitudinalObservationRecord {
    LongitudinalObservationSet::new()
        .ingest(LongitudinalObservationInput {
            observation_record_ref,
            enrollment_ref,
            source_system_ref,
            source_observation_ref,
            construct_ref,
            measure_ref,
            membership_shares: memberships,
            time: ObservationTimeInput {
                validity_start_at_unix_ms,
                validity_end_at_unix_ms,
                recorded_at_unix_ms,
                received_at_unix_ms,
                ingested_at_unix_ms,
                timezone_name,
                utc_offset_minutes,
            },
        })
        .unwrap()
}

fn base_record() -> LongitudinalObservationRecord {
    let memberships = [
        MembershipShareInput {
            membership_context_ref: "clinic_ward_seoul_01",
            weight_parts_per_10_000: 6_000,
        },
        MembershipShareInput {
            membership_context_ref: "night_shift_team_alpha",
            weight_parts_per_10_000: 4_000,
        },
    ];
    record(
        BASE_RECORD_REF,
        BASE_ENROLLMENT_REF,
        BASE_SOURCE_SYSTEM_REF,
        BASE_SOURCE_OBSERVATION_REF,
        BASE_CONSTRUCT_REF,
        BASE_MEASURE_REF,
        &memberships,
        BASE_VALIDITY_START,
        BASE_VALIDITY_END,
        BASE_RECORDED_AT,
        BASE_RECEIVED_AT,
        BASE_INGESTED_AT,
        BASE_TIMEZONE,
        BASE_OFFSET,
    )
}

fn variant(
    observation_record_ref: &str,
    enrollment_ref: &str,
    source_system_ref: &str,
    source_observation_ref: &str,
    construct_ref: &str,
    measure_ref: &str,
    memberships: &[MembershipShareInput<'_>],
    validity_start_at_unix_ms: u64,
    validity_end_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
    timezone_name: &str,
    utc_offset_minutes: i16,
) -> LongitudinalObservationRecord {
    record(
        observation_record_ref,
        enrollment_ref,
        source_system_ref,
        source_observation_ref,
        construct_ref,
        measure_ref,
        memberships,
        validity_start_at_unix_ms,
        validity_end_at_unix_ms,
        recorded_at_unix_ms,
        received_at_unix_ms,
        ingested_at_unix_ms,
        timezone_name,
        utc_offset_minutes,
    )
}

fn base_memberships() -> [MembershipShareInput<'static>; 2] {
    [
        MembershipShareInput {
            membership_context_ref: "clinic_ward_seoul_01",
            weight_parts_per_10_000: 6_000,
        },
        MembershipShareInput {
            membership_context_ref: "night_shift_team_alpha",
            weight_parts_per_10_000: 4_000,
        },
    ]
}

fn persist(
    client: &mut Client,
    tenant_ref: &str,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    let mut transaction = client.transaction().unwrap();
    let result = persist_longitudinal_observation(&mut transaction, tenant_ref, record);
    match result {
        Ok(disposition) => {
            transaction.commit().unwrap();
            Ok(disposition)
        }
        Err(error) => {
            transaction.rollback().unwrap();
            Err(error)
        }
    }
}

fn assert_conflict(
    client: &mut Client,
    tenant_ref: &str,
    candidate: &LongitudinalObservationRecord,
) {
    assert!(matches!(
        persist(client, tenant_ref, candidate),
        Err(LongitudinalObservationPersistenceError::ConflictingReplay)
    ));
}

#[test]
fn every_immutable_header_and_membership_dimension_rejects_rebinding() {
    let _guard = guard();
    let mut client = client();
    let base = base_record();
    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &base).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &base).unwrap(),
        LongitudinalObservationPersistenceDisposition::Duplicate
    );

    let memberships = base_memberships();
    let header_variants = [
        variant(
            "longitudinal_observation_record_other",
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            "longitudinal_enrollment_other",
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            "gyeot_collection_other",
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            "gyeot_observation_other",
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            "construct_agreeableness",
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            "measure_ipip_extraversion_ko_v2",
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START + 1,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END + 1,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT + 1,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT + 1,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT + 1,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            "Asia/Tokyo",
            BASE_OFFSET,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECORDED_AT,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            480,
        ),
        variant(
            BASE_RECORD_REF,
            BASE_ENROLLMENT_REF,
            BASE_SOURCE_SYSTEM_REF,
            BASE_SOURCE_OBSERVATION_REF,
            BASE_CONSTRUCT_REF,
            BASE_MEASURE_REF,
            &memberships,
            BASE_VALIDITY_START,
            BASE_VALIDITY_END,
            BASE_RECEIVED_AT + 1,
            BASE_RECEIVED_AT,
            BASE_INGESTED_AT,
            BASE_TIMEZONE,
            BASE_OFFSET,
        ),
    ];
    for candidate in &header_variants {
        assert_conflict(&mut client, "tenant_clinic_seoul", candidate);
    }
    assert_conflict(&mut client, "tenant_clinic_busan", &base);

    let one_membership = [MembershipShareInput {
        membership_context_ref: "clinic_ward_seoul_01",
        weight_parts_per_10_000: 10_000,
    }];
    let membership_count_mismatch = variant(
        BASE_RECORD_REF,
        BASE_ENROLLMENT_REF,
        BASE_SOURCE_SYSTEM_REF,
        BASE_SOURCE_OBSERVATION_REF,
        BASE_CONSTRUCT_REF,
        BASE_MEASURE_REF,
        &one_membership,
        BASE_VALIDITY_START,
        BASE_VALIDITY_END,
        BASE_RECORDED_AT,
        BASE_RECEIVED_AT,
        BASE_INGESTED_AT,
        BASE_TIMEZONE,
        BASE_OFFSET,
    );
    assert_conflict(
        &mut client,
        "tenant_clinic_seoul",
        &membership_count_mismatch,
    );

    let context_mismatch_memberships = [
        MembershipShareInput {
            membership_context_ref: "clinic_ward_seoul_02",
            weight_parts_per_10_000: 6_000,
        },
        MembershipShareInput {
            membership_context_ref: "night_shift_team_alpha",
            weight_parts_per_10_000: 4_000,
        },
    ];
    let context_mismatch = variant(
        BASE_RECORD_REF,
        BASE_ENROLLMENT_REF,
        BASE_SOURCE_SYSTEM_REF,
        BASE_SOURCE_OBSERVATION_REF,
        BASE_CONSTRUCT_REF,
        BASE_MEASURE_REF,
        &context_mismatch_memberships,
        BASE_VALIDITY_START,
        BASE_VALIDITY_END,
        BASE_RECORDED_AT,
        BASE_RECEIVED_AT,
        BASE_INGESTED_AT,
        BASE_TIMEZONE,
        BASE_OFFSET,
    );
    assert_conflict(&mut client, "tenant_clinic_seoul", &context_mismatch);

    let weight_mismatch_memberships = [
        MembershipShareInput {
            membership_context_ref: "clinic_ward_seoul_01",
            weight_parts_per_10_000: 5_000,
        },
        MembershipShareInput {
            membership_context_ref: "night_shift_team_alpha",
            weight_parts_per_10_000: 5_000,
        },
    ];
    let weight_mismatch = variant(
        BASE_RECORD_REF,
        BASE_ENROLLMENT_REF,
        BASE_SOURCE_SYSTEM_REF,
        BASE_SOURCE_OBSERVATION_REF,
        BASE_CONSTRUCT_REF,
        BASE_MEASURE_REF,
        &weight_mismatch_memberships,
        BASE_VALIDITY_START,
        BASE_VALIDITY_END,
        BASE_RECORDED_AT,
        BASE_RECEIVED_AT,
        BASE_INGESTED_AT,
        BASE_TIMEZONE,
        BASE_OFFSET,
    );
    assert_conflict(&mut client, "tenant_clinic_seoul", &weight_mismatch);

    let busan_record = variant(
        "longitudinal_observation_record_busan_source_alias",
        BASE_ENROLLMENT_REF,
        BASE_SOURCE_SYSTEM_REF,
        BASE_SOURCE_OBSERVATION_REF,
        BASE_CONSTRUCT_REF,
        BASE_MEASURE_REF,
        &memberships,
        BASE_VALIDITY_START,
        BASE_VALIDITY_END,
        BASE_RECORDED_AT,
        BASE_RECEIVED_AT,
        BASE_INGESTED_AT,
        BASE_TIMEZONE,
        BASE_OFFSET,
    );
    assert_eq!(
        persist(&mut client, "tenant_clinic_busan", &busan_record).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert_conflict(&mut client, "tenant_clinic_busan", &base);
}

#[test]
fn corrupted_sequence_and_anomaly_evidence_fail_closed_after_restart() {
    let _guard = guard();
    let mut client = client();
    let base = base_record();
    persist(&mut client, "tenant_clinic_seoul", &base).unwrap();

    client
        .batch_execute(
            "ALTER TABLE longitudinal_membership_share \
             DISABLE TRIGGER longitudinal_membership_immutable_update; \
             UPDATE longitudinal_membership_share SET membership_sequence = 3 \
             WHERE observation_record_ref = 'longitudinal_observation_record_coverage' \
               AND membership_sequence = 1; \
             ALTER TABLE longitudinal_membership_share \
             ENABLE TRIGGER longitudinal_membership_immutable_update;",
        )
        .unwrap();
    assert_conflict(&mut client, "tenant_clinic_seoul", &base);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_longitudinal_observation(&mut transaction, "tenant_clinic_seoul", BASE_RECORD_REF),
        Err(LongitudinalObservationPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();

    let mut client = client();
    let base = base_record();
    persist(&mut client, "tenant_clinic_seoul", &base).unwrap();
    client
        .batch_execute(
            "ALTER TABLE longitudinal_observation \
             DISABLE TRIGGER longitudinal_observation_immutable_update; \
             UPDATE longitudinal_observation \
             SET clock_anomaly_code = 'recorded_after_received' \
             WHERE observation_record_ref = 'longitudinal_observation_record_coverage'; \
             ALTER TABLE longitudinal_observation \
             ENABLE TRIGGER longitudinal_observation_immutable_update;",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_longitudinal_observation(&mut transaction, "tenant_clinic_seoul", BASE_RECORD_REF),
        Err(LongitudinalObservationPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_longitudinal_observation(&mut transaction, "tenant_clinic_seoul", " 123 "),
        Err(LongitudinalObservationPersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn all_public_error_variants_have_stable_messages_and_database_sources() {
    for error in [
        LongitudinalObservationPersistenceError::InvalidReference,
        LongitudinalObservationPersistenceError::InvalidNumericRange,
        LongitudinalObservationPersistenceError::ConflictingReplay,
        LongitudinalObservationPersistenceError::CorruptHistory,
        LongitudinalObservationPersistenceError::UnsupportedIsolationLevel,
    ] {
        assert!(!error.to_string().is_empty());
        assert!(Error::source(&error).is_none());
    }

    let _guard = guard();
    let mut client = client();
    let base = base_record();
    client
        .batch_execute("DROP SCHEMA longitudinal_observation_replay_coverage_test CASCADE")
        .unwrap();
    let error = persist(&mut client, "tenant_clinic_seoul", &base)
        .expect_err("missing persistence schema must surface the PostgreSQL source");
    assert!(matches!(
        error,
        LongitudinalObservationPersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL longitudinal observation persistence failed"
    );
    assert!(Error::source(&error).is_some());
}
