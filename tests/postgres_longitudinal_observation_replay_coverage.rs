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

#[derive(Clone, Copy)]
struct ObservationSpec<'a> {
    observation_record_ref: &'a str,
    enrollment_ref: &'a str,
    source_system_ref: &'a str,
    source_observation_ref: &'a str,
    construct_ref: &'a str,
    measure_ref: &'a str,
    memberships: &'a [MembershipShareInput<'a>],
    validity_start_at_unix_ms: u64,
    validity_end_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
    timezone_name: &'a str,
    utc_offset_minutes: i16,
}

impl<'a> ObservationSpec<'a> {
    fn base(memberships: &'a [MembershipShareInput<'a>]) -> Self {
        Self {
            observation_record_ref: BASE_RECORD_REF,
            enrollment_ref: BASE_ENROLLMENT_REF,
            source_system_ref: BASE_SOURCE_SYSTEM_REF,
            source_observation_ref: BASE_SOURCE_OBSERVATION_REF,
            construct_ref: BASE_CONSTRUCT_REF,
            measure_ref: BASE_MEASURE_REF,
            memberships,
            validity_start_at_unix_ms: BASE_VALIDITY_START,
            validity_end_at_unix_ms: BASE_VALIDITY_END,
            recorded_at_unix_ms: BASE_RECORDED_AT,
            received_at_unix_ms: BASE_RECEIVED_AT,
            ingested_at_unix_ms: BASE_INGESTED_AT,
            timezone_name: BASE_TIMEZONE,
            utc_offset_minutes: BASE_OFFSET,
        }
    }

    fn build(self) -> LongitudinalObservationRecord {
        LongitudinalObservationSet::new()
            .ingest(LongitudinalObservationInput {
                observation_record_ref: self.observation_record_ref,
                enrollment_ref: self.enrollment_ref,
                source_system_ref: self.source_system_ref,
                source_observation_ref: self.source_observation_ref,
                construct_ref: self.construct_ref,
                measure_ref: self.measure_ref,
                membership_shares: self.memberships,
                time: ObservationTimeInput {
                    validity_start_at_unix_ms: self.validity_start_at_unix_ms,
                    validity_end_at_unix_ms: self.validity_end_at_unix_ms,
                    recorded_at_unix_ms: self.recorded_at_unix_ms,
                    received_at_unix_ms: self.received_at_unix_ms,
                    ingested_at_unix_ms: self.ingested_at_unix_ms,
                    timezone_name: self.timezone_name,
                    utc_offset_minutes: self.utc_offset_minutes,
                },
            })
            .unwrap()
    }
}

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn fresh_client() -> Client {
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

fn base_record() -> LongitudinalObservationRecord {
    let memberships = base_memberships();
    ObservationSpec::base(&memberships).build()
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

fn assert_identity_conflict(
    client: &mut Client,
    tenant_ref: &str,
    candidate: &LongitudinalObservationRecord,
) {
    assert!(matches!(
        persist(client, tenant_ref, candidate),
        Err(LongitudinalObservationPersistenceError::ObservationIdentityConflict)
    ));
}

#[test]
fn every_immutable_header_dimension_rejects_rebinding() {
    let _guard = guard();
    let mut client = fresh_client();
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
    let spec = ObservationSpec::base(&memberships);
    let source_identity_variants = [
        ObservationSpec {
            enrollment_ref: "longitudinal_enrollment_other",
            ..spec
        }
        .build(),
        ObservationSpec {
            source_system_ref: "gyeot_collection_other",
            ..spec
        }
        .build(),
        ObservationSpec {
            source_observation_ref: "gyeot_observation_other",
            ..spec
        }
        .build(),
    ];
    for candidate in &source_identity_variants {
        assert_identity_conflict(&mut client, "tenant_clinic_seoul", candidate);
    }

    let replay_variants = [
        ObservationSpec {
            observation_record_ref: "longitudinal_observation_record_other",
            ..spec
        }
        .build(),
        ObservationSpec {
            construct_ref: "construct_agreeableness",
            ..spec
        }
        .build(),
        ObservationSpec {
            measure_ref: "measure_ipip_extraversion_ko_v2",
            ..spec
        }
        .build(),
        ObservationSpec {
            validity_start_at_unix_ms: BASE_VALIDITY_START + 1,
            ..spec
        }
        .build(),
        ObservationSpec {
            validity_end_at_unix_ms: BASE_VALIDITY_END + 1,
            ..spec
        }
        .build(),
        ObservationSpec {
            recorded_at_unix_ms: BASE_RECORDED_AT + 1,
            ..spec
        }
        .build(),
        ObservationSpec {
            received_at_unix_ms: BASE_RECEIVED_AT + 1,
            ..spec
        }
        .build(),
        ObservationSpec {
            ingested_at_unix_ms: BASE_INGESTED_AT + 1,
            ..spec
        }
        .build(),
        ObservationSpec {
            timezone_name: "Asia/Tokyo",
            ..spec
        }
        .build(),
        ObservationSpec {
            utc_offset_minutes: 480,
            ..spec
        }
        .build(),
        ObservationSpec {
            recorded_at_unix_ms: BASE_RECEIVED_AT + 1,
            ..spec
        }
        .build(),
    ];
    for candidate in &replay_variants {
        assert_conflict(&mut client, "tenant_clinic_seoul", candidate);
    }
    assert_conflict(&mut client, "tenant_clinic_busan", &base);
}

#[test]
fn every_membership_dimension_and_source_alias_rejects_rebinding() {
    let _guard = guard();
    let mut client = fresh_client();
    let base = base_record();
    assert_eq!(
        persist(&mut client, "tenant_clinic_seoul", &base).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );

    let one_membership = [MembershipShareInput {
        membership_context_ref: "clinic_ward_seoul_01",
        weight_parts_per_10_000: 10_000,
    }];
    let one_membership_record = ObservationSpec::base(&one_membership).build();
    assert_conflict(&mut client, "tenant_clinic_seoul", &one_membership_record);

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
    let context_mismatch = ObservationSpec::base(&context_mismatch_memberships).build();
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
    let weight_mismatch = ObservationSpec::base(&weight_mismatch_memberships).build();
    assert_conflict(&mut client, "tenant_clinic_seoul", &weight_mismatch);

    let memberships = base_memberships();
    let busan_record = ObservationSpec {
        observation_record_ref: "longitudinal_observation_record_busan_source_alias",
        ..ObservationSpec::base(&memberships)
    }
    .build();
    assert_eq!(
        persist(&mut client, "tenant_clinic_busan", &busan_record).unwrap(),
        LongitudinalObservationPersistenceDisposition::Inserted
    );
    assert_conflict(&mut client, "tenant_clinic_busan", &base);
}

#[test]
fn corrupted_sequence_and_anomaly_evidence_fail_closed_after_restart() {
    let _guard = guard();
    let mut client = fresh_client();
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

    let mut client = fresh_client();
    let base = base_record();
    persist(&mut client, "tenant_clinic_seoul", &base).unwrap();
    client
        .batch_execute(
            "ALTER TABLE longitudinal_observation \
             DISABLE TRIGGER longitudinal_observation_immutable_update; \
             ALTER TABLE longitudinal_observation \
             DROP CONSTRAINT longitudinal_observation_anomaly_check; \
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
    let mut client = fresh_client();
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
