//! Real-PostgreSQL regression for durable Commons longitudinal record identity.
//!
//! The in-memory aggregate rejects a distinct Gyeot source observation that reuses
//! one Commons `observation_record_ref`. Persistence must preserve the same immutable
//! identity boundary across process restarts instead of allowing the second source to
//! replace or alias the first accepted record.

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
            "DROP SCHEMA IF EXISTS longitudinal_observation_record_identity_test CASCADE; \
             CREATE SCHEMA longitudinal_observation_record_identity_test; \
             SET search_path TO longitudinal_observation_record_identity_test;",
        )
        .unwrap();
    apply_longitudinal_observation_migration(&mut client).unwrap();
    client
}

fn record(
    observation_record_ref: &str,
    source_observation_ref: &str,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
) -> LongitudinalObservationRecord {
    let memberships = [MembershipShareInput {
        membership_context_ref: "clinic_ward_seoul_01",
        weight_parts_per_10_000: 10_000,
    }];
    LongitudinalObservationSet::new()
        .ingest(LongitudinalObservationInput {
            observation_record_ref,
            enrollment_ref: "longitudinal_enrollment_identity_001",
            source_system_ref: "gyeot_mobile_collection",
            source_observation_ref,
            construct_ref: "construct_anxious_mood",
            measure_ref: "measure_anxious_mood_ema_v3",
            membership_shares: &memberships,
            time: ObservationTimeInput {
                validity_start_at_unix_ms: received_at_unix_ms - 2_000,
                validity_end_at_unix_ms: received_at_unix_ms - 2_000,
                recorded_at_unix_ms: received_at_unix_ms - 1_000,
                received_at_unix_ms,
                ingested_at_unix_ms,
                timezone_name: "Asia/Seoul",
                utc_offset_minutes: 540,
            },
        })
        .expect("fixture must satisfy the domain contract")
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

fn load(
    client: &mut Client,
    tenant_ref: &str,
    observation_record_ref: &str,
) -> Result<Option<LongitudinalObservationRecord>, LongitudinalObservationPersistenceError> {
    let mut transaction = client.transaction().unwrap();
    let result =
        load_longitudinal_observation(&mut transaction, tenant_ref, observation_record_ref);
    match result {
        Ok(record) => {
            transaction.commit().unwrap();
            Ok(record)
        }
        Err(error) => {
            transaction.rollback().unwrap();
            Err(error)
        }
    }
}

#[test]
fn distinct_sources_cannot_rebind_one_durable_commons_record_identity() {
    let _guard = guard();
    let mut client = client();
    let first = record(
        "longitudinal_observation_record_shared_identity",
        "gyeot_observation_identity_first",
        1_776_336_120_000,
        1_776_336_180_000,
    );
    let collision = record(
        "longitudinal_observation_record_shared_identity",
        "gyeot_observation_identity_second",
        1_776_336_240_000,
        1_776_336_300_000,
    );

    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &first).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert!(matches!(
        persist(&mut client, "tenant_clinic_seoul", &collision),
        Err(LongitudinalObservationPersistenceError::ObservationIdentityConflict)
    ));

    assert_eq!(
        load(
            &mut client,
            "tenant_clinic_seoul",
            "longitudinal_observation_record_shared_identity",
        )
        .unwrap(),
        Some(first)
    );

    let observation_count: i64 = client
        .query_one("SELECT count(*) FROM longitudinal_observation", &[])
        .unwrap()
        .get(0);
    let membership_count: i64 = client
        .query_one("SELECT count(*) FROM longitudinal_membership_share", &[])
        .unwrap()
        .get(0);
    assert_eq!(observation_count, 1);
    assert_eq!(membership_count, 1);
}

#[test]
fn another_tenant_cannot_rebind_one_durable_commons_record_identity() {
    let _guard = guard();
    let mut client = client();
    let first = record(
        "longitudinal_observation_record_cross_tenant_identity",
        "gyeot_observation_cross_tenant_identity",
        1_776_336_360_000,
        1_776_336_420_000,
    );
    let collision = record(
        "longitudinal_observation_record_cross_tenant_identity",
        "gyeot_observation_cross_tenant_identity",
        1_776_336_360_000,
        1_776_336_420_000,
    );

    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &first).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert!(matches!(
        persist(&mut client, "tenant_clinic_busan", &collision),
        Err(LongitudinalObservationPersistenceError::ObservationIdentityConflict)
    ));

    assert_eq!(
        load(
            &mut client,
            "tenant_clinic_seoul",
            "longitudinal_observation_record_cross_tenant_identity",
        )
        .unwrap(),
        Some(first)
    );
    assert_eq!(
        load(
            &mut client,
            "tenant_clinic_busan",
            "longitudinal_observation_record_cross_tenant_identity",
        )
        .unwrap(),
        None
    );

    let observation_count: i64 = client
        .query_one("SELECT count(*) FROM longitudinal_observation", &[])
        .unwrap()
        .get(0);
    let membership_count: i64 = client
        .query_one("SELECT count(*) FROM longitudinal_membership_share", &[])
        .unwrap()
        .get(0);
    assert_eq!(observation_count, 1);
    assert_eq!(membership_count, 1);
}
