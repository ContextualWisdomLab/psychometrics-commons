//! Real `PostgreSQL` contract for durable longitudinal observation evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::longitudinal_observation::{
    LongitudinalObservationInput, LongitudinalObservationRecord, LongitudinalObservationSet,
    MembershipShareInput, ObservationTimeInput,
};
use psychometrics_commons_runtime::postgres_longitudinal_observation::{
    apply_longitudinal_observation_migration, persist_longitudinal_observation,
    LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

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
            "CREATE SCHEMA IF NOT EXISTS longitudinal_observation_persistence_test; \
             SET search_path TO longitudinal_observation_persistence_test;",
        )
        .unwrap();
    client
}

fn reset(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS longitudinal_observation_persistence_test.longitudinal_membership_share CASCADE; \
             DROP TABLE IF EXISTS longitudinal_observation_persistence_test.longitudinal_observation CASCADE; \
             DROP FUNCTION IF EXISTS longitudinal_observation_persistence_test.reject_longitudinal_mutation() CASCADE; \
             DROP FUNCTION IF EXISTS longitudinal_observation_persistence_test.enforce_longitudinal_membership_total() CASCADE;",
        )
        .unwrap();
}

fn observation(record_ref: &str, construct_ref: &str) -> LongitudinalObservationRecord {
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
    LongitudinalObservationSet::new()
        .ingest(LongitudinalObservationInput {
            observation_record_ref: record_ref,
            enrollment_ref: "longitudinal_enrollment_ko_001",
            source_system_ref: "gyeot_mobile_collection",
            source_observation_ref: "gyeot_observation_20260818_001",
            construct_ref,
            measure_ref: "measure_ipip_extraversion_ko_v1",
            membership_shares: &memberships,
            time: ObservationTimeInput {
                validity_start_at_unix_ms: 1_776_661_900_000,
                validity_end_at_unix_ms: 1_776_662_200_000,
                recorded_at_unix_ms: 1_776_662_200_000,
                received_at_unix_ms: 1_776_662_260_000,
                ingested_at_unix_ms: 1_776_662_270_000,
                timezone_name: "Asia/Seoul",
                utc_offset_minutes: 540,
            },
        })
        .unwrap()
}

fn persist(
    client: &mut Client,
    tenant_ref: &str,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    let mut tx = client.transaction().unwrap();
    let result = persist_longitudinal_observation(&mut tx, tenant_ref, record);
    match result {
        Ok(disposition) => {
            tx.commit().unwrap();
            Ok(disposition)
        }
        Err(error) => {
            tx.rollback().unwrap();
            Err(error)
        }
    }
}

#[test]
fn seoul_multiple_membership_observation_is_tenant_bound_durable_and_idempotent() {
    let _guard = guard();
    let mut client = client();
    reset(&mut client);
    apply_longitudinal_observation_migration(&mut client).unwrap();
    let record = observation(
        "longitudinal_observation_record_001",
        "construct_extraversion",
    );
    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &record).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &record).unwrap(),
        LongitudinalObservationPersistenceDisposition::Duplicate
    );
    let row = client
        .query_one(
            "SELECT tenant_ref, timezone_name, utc_offset_minutes \
             FROM longitudinal_observation WHERE observation_record_ref = $1",
            &[&record.observation_record_ref()],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "tenant_clinic_seoul");
    assert_eq!(row.get::<_, String>(1), "Asia/Seoul");
    assert_eq!(row.get::<_, i16>(2), 540);
    let memberships = client
        .query(
            "SELECT membership_context_ref, weight_parts_per_10_000 \
             FROM longitudinal_membership_share WHERE observation_record_ref = $1 \
             ORDER BY membership_sequence",
            &[&record.observation_record_ref()],
        )
        .unwrap();
    assert_eq!(memberships.len(), 2);
    assert_eq!(memberships[0].get::<_, String>(0), "clinic_ward_seoul_01");
    assert_eq!(memberships[0].get::<_, i32>(1), 6_000);
    assert_eq!(memberships[1].get::<_, String>(0), "night_shift_team_alpha");
    assert_eq!(memberships[1].get::<_, i32>(1), 4_000);
}

#[test]
fn tenant_and_source_rebinding_fail_closed_without_cross_tenant_aliasing() {
    let _guard = guard();
    let mut client = client();
    reset(&mut client);
    apply_longitudinal_observation_migration(&mut client).unwrap();
    let first = observation(
        "longitudinal_observation_record_002",
        "construct_extraversion",
    );
    persist(&mut client, "tenant_clinic_seoul", &first).unwrap();

    assert!(matches!(
        persist(&mut client, " tenant_clinic_seoul ", &first),
        Err(LongitudinalObservationPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        persist(&mut client, "tenant_clinic_busan", &first),
        Err(LongitudinalObservationPersistenceError::ConflictingReplay)
    ));

    let rebound = observation(
        "longitudinal_observation_record_rebound",
        "construct_agreeableness",
    );
    assert!(matches!(
        persist(&mut client, "tenant_clinic_seoul", &rebound),
        Err(LongitudinalObservationPersistenceError::ConflictingReplay)
    ));
    assert_eq!(
        persist(&mut client, "tenant_clinic_busan", &rebound).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );

    let update_error = client
        .execute(
            "UPDATE longitudinal_observation SET construct_ref = 'construct_rebound' \
             WHERE observation_record_ref = $1",
            &[&first.observation_record_ref()],
        )
        .unwrap_err();
    assert_eq!(
        update_error
            .as_db_error()
            .map(postgres::error::DbError::code),
        Some(&postgres::error::SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
    );
    let delete_error = client
        .execute(
            "DELETE FROM longitudinal_membership_share WHERE observation_record_ref = $1",
            &[&first.observation_record_ref()],
        )
        .unwrap_err();
    assert_eq!(
        delete_error
            .as_db_error()
            .map(postgres::error::DbError::code),
        Some(&postgres::error::SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
    );
}

#[test]
fn persistence_requires_read_committed_and_live_schema() {
    let _guard = guard();
    let mut client = client();
    reset(&mut client);
    apply_longitudinal_observation_migration(&mut client).unwrap();
    let record = observation(
        "longitudinal_observation_record_003",
        "construct_extraversion",
    );
    let mut tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_longitudinal_observation(&mut tx, "tenant_clinic_seoul", &record),
        Err(LongitudinalObservationPersistenceError::UnsupportedIsolationLevel)
    ));
    tx.rollback().unwrap();
    reset(&mut client);
    assert!(matches!(
        persist(&mut client, "tenant_clinic_seoul", &record),
        Err(LongitudinalObservationPersistenceError::Database(_))
    ));
}
