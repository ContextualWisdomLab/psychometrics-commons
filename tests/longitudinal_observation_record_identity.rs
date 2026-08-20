//! Regression contract for unique Commons longitudinal observation identities.

use psychometrics_commons_runtime::longitudinal_observation::{
    LongitudinalObservationError, LongitudinalObservationInput, LongitudinalObservationSet,
    MembershipShareInput, ObservationTimeInput,
};

static MEMBERSHIPS: [MembershipShareInput<'static>; 1] = [MembershipShareInput {
    membership_context_ref: "seoul_clinic_ward_a",
    weight_parts_per_10_000: 10_000,
}];

fn observation<'a>(
    observation_record_ref: &'a str,
    source_observation_ref: &'a str,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
) -> LongitudinalObservationInput<'a> {
    LongitudinalObservationInput {
        observation_record_ref,
        enrollment_ref: "enrollment_seoul_clinic_2026q3",
        source_system_ref: "gyeot_ema_android",
        source_observation_ref,
        construct_ref: "construct_anxious_mood",
        measure_ref: "measure_anxious_mood_ema_v3",
        membership_shares: &MEMBERSHIPS,
        time: ObservationTimeInput {
            validity_start_at_unix_ms: 1_776_297_000_000,
            validity_end_at_unix_ms: 1_776_297_000_000,
            recorded_at_unix_ms: received_at_unix_ms - 1_000,
            received_at_unix_ms,
            ingested_at_unix_ms,
            timezone_name: "Asia/Seoul",
            utc_offset_minutes: 540,
        },
    }
}

#[test]
fn distinct_source_observations_cannot_reuse_one_commons_record_identity() {
    let mut observations = LongitudinalObservationSet::new();
    observations
        .ingest(observation(
            "observation_record_shared_identity",
            "gyeot_obs_first",
            1_776_336_120_000,
            1_776_336_180_000,
        ))
        .expect("first source observation owns the Commons record identity");

    let conflict = observations
        .ingest(observation(
            "observation_record_shared_identity",
            "gyeot_obs_second",
            1_776_336_240_000,
            1_776_336_300_000,
        ))
        .expect_err("a distinct source observation must not reuse the Commons record identity");

    assert_eq!(
        conflict,
        LongitudinalObservationError::ObservationIdentityConflict
    );
    assert!(conflict
        .to_string()
        .contains("observation-record identity"));
    assert_eq!(observations.len(), 1);
}
