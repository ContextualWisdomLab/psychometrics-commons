//! Realistic Seoul-clinic EMA ingest: keep source and platform clocks distinct
//! and refuse a single primary-group collapse.

use psychometrics_commons_runtime::longitudinal_observation::{
    ClockAnomaly, LongitudinalObservationError, LongitudinalObservationInput,
    LongitudinalObservationSet, MembershipShareInput, ObservationTimeInput,
};

/// Evening anxious-mood report written on-device while the phone was offline.
const OBSERVED_AT_UNIX_MS: u64 = 1_776_297_000_000;
/// Same instant the Gyeot client encrypted the observation locally.
const RECORDED_AT_UNIX_MS: u64 = 1_776_297_000_000;
/// Next-morning sync when Commons first saw the candidate.
const RECEIVED_AT_UNIX_MS: u64 = 1_776_336_120_000;
/// Durable accept a minute later. Must not replace observed time.
const INGESTED_AT_UNIX_MS: u64 = 1_776_336_180_000;
const SEOUL_OFFSET_MINUTES: i16 = 540;

fn seoul_clinic_shares() -> [MembershipShareInput<'static>; 2] {
    [
        MembershipShareInput {
            membership_context_ref: "seoul_clinic_ward_a",
            weight_parts_per_10_000: 6_000,
        },
        MembershipShareInput {
            membership_context_ref: "evening_shift_team",
            weight_parts_per_10_000: 4_000,
        },
    ]
}

fn evening_mood_times() -> ObservationTimeInput<'static> {
    ObservationTimeInput {
        validity_start_at_unix_ms: OBSERVED_AT_UNIX_MS,
        validity_end_at_unix_ms: OBSERVED_AT_UNIX_MS,
        recorded_at_unix_ms: RECORDED_AT_UNIX_MS,
        received_at_unix_ms: RECEIVED_AT_UNIX_MS,
        ingested_at_unix_ms: INGESTED_AT_UNIX_MS,
        timezone_name: "Asia/Seoul",
        utc_offset_minutes: SEOUL_OFFSET_MINUTES,
    }
}

fn evening_mood_input<'a>(
    observation_record_ref: &'a str,
    source_observation_ref: &'a str,
    membership_shares: &'a [MembershipShareInput<'a>],
    time: ObservationTimeInput<'a>,
) -> LongitudinalObservationInput<'a> {
    LongitudinalObservationInput {
        observation_record_ref,
        enrollment_ref: "enrollment_seoul_clinic_2026q3",
        source_system_ref: "gyeot_ema_android",
        source_observation_ref,
        construct_ref: "construct_anxious_mood",
        measure_ref: "measure_anxious_mood_ema_v3",
        membership_shares,
        time,
    }
}

#[test]
fn offline_seoul_clinic_ingest_keeps_four_clocks_and_both_memberships() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();
    let accepted = observations
        .ingest(evening_mood_input(
            "observation_record_evening_001",
            "gyeot_obs_phone_88f2",
            &shares,
            evening_mood_times(),
        ))
        .expect("offline evening mood should ingest");

    assert_eq!(
        accepted.observation_record_ref(),
        "observation_record_evening_001"
    );
    assert_eq!(accepted.enrollment_ref(), "enrollment_seoul_clinic_2026q3");
    assert_eq!(accepted.source_system_ref(), "gyeot_ema_android");
    assert_eq!(accepted.source_observation_ref(), "gyeot_obs_phone_88f2");
    assert_eq!(accepted.construct_ref(), "construct_anxious_mood");
    assert_eq!(accepted.measure_ref(), "measure_anxious_mood_ema_v3");
    assert_eq!(accepted.validity_start_at_unix_ms(), OBSERVED_AT_UNIX_MS);
    assert_eq!(accepted.validity_end_at_unix_ms(), OBSERVED_AT_UNIX_MS);
    assert_eq!(accepted.recorded_at_unix_ms(), RECORDED_AT_UNIX_MS);
    assert_eq!(accepted.received_at_unix_ms(), RECEIVED_AT_UNIX_MS);
    assert_eq!(accepted.ingested_at_unix_ms(), INGESTED_AT_UNIX_MS);
    assert_eq!(accepted.timezone_name(), "Asia/Seoul");
    assert_eq!(accepted.utc_offset_minutes(), SEOUL_OFFSET_MINUTES);
    assert_eq!(accepted.clock_anomaly(), None);
    assert_eq!(
        accepted
            .membership_shares()
            .iter()
            .map(|share| (
                share.membership_context_ref(),
                share.weight_parts_per_10_000()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("seoul_clinic_ward_a", 6_000),
            ("evening_shift_team", 4_000),
        ]
    );
    assert_eq!(observations.len(), 1);
    assert!(!observations.is_empty());
}

#[test]
fn exact_source_replay_is_idempotent_and_conflict_fails_closed() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();
    let first = observations
        .ingest(evening_mood_input(
            "observation_record_evening_001",
            "gyeot_obs_phone_88f2",
            &shares,
            evening_mood_times(),
        ))
        .expect("first ingest");

    let replay = observations
        .ingest(evening_mood_input(
            "observation_record_evening_001",
            "gyeot_obs_phone_88f2",
            &shares,
            evening_mood_times(),
        ))
        .expect("exact Gyeot retry must not create a second row");
    assert_eq!(replay, first);
    assert_eq!(observations.len(), 1);

    let mut later = evening_mood_times();
    later.ingested_at_unix_ms = INGESTED_AT_UNIX_MS + 1;
    let conflict = observations
        .ingest(evening_mood_input(
            "observation_record_evening_002",
            "gyeot_obs_phone_88f2",
            &shares,
            later,
        ))
        .expect_err("same phone observation with a new ingest clock must not last-write-win");
    assert_eq!(conflict, LongitudinalObservationError::IdempotencyConflict);
    assert!(conflict
        .to_string()
        .contains("replay the first accepted observation"));
}

#[test]
fn collapsing_to_one_primary_group_is_rejected() {
    let shares = [MembershipShareInput {
        membership_context_ref: "seoul_clinic_ward_a",
        weight_parts_per_10_000: 10_000,
    }];
    let mut observations = LongitudinalObservationSet::new();
    let collapsed = observations.ingest(evening_mood_input(
        "observation_record_evening_001",
        "gyeot_obs_phone_88f2",
        &shares,
        evening_mood_times(),
    ));
    // A single 100% share is legal when the study declared one context.
    assert!(collapsed.is_ok());

    let empty = observations
        .ingest(evening_mood_input(
            "observation_record_evening_002",
            "gyeot_obs_phone_88f3",
            &[],
            evening_mood_times(),
        ))
        .expect_err("an observation without membership is a collapsed atomistic row");
    assert_eq!(empty, LongitudinalObservationError::EmptyMembership);
    assert!(empty
        .to_string()
        .contains("declare every membership context"));

    let unbalanced = [
        MembershipShareInput {
            membership_context_ref: "seoul_clinic_ward_a",
            weight_parts_per_10_000: 6_000,
        },
        MembershipShareInput {
            membership_context_ref: "evening_shift_team",
            weight_parts_per_10_000: 3_000,
        },
    ];
    let sum = observations
        .ingest(evening_mood_input(
            "observation_record_evening_003",
            "gyeot_obs_phone_88f4",
            &unbalanced,
            evening_mood_times(),
        ))
        .expect_err("partial weights would silently invent a primary remainder");
    assert_eq!(sum, LongitudinalObservationError::MembershipWeightsDoNotSum);

    let zero = [MembershipShareInput {
        membership_context_ref: "seoul_clinic_ward_a",
        weight_parts_per_10_000: 0,
    }];
    let zero_weight = observations
        .ingest(evening_mood_input(
            "observation_record_evening_004",
            "gyeot_obs_phone_88f5",
            &zero,
            evening_mood_times(),
        ))
        .expect_err("zero-weight context is a hidden drop");
    assert_eq!(
        zero_weight,
        LongitudinalObservationError::InvalidMembershipWeight
    );

    let duplicates = [
        MembershipShareInput {
            membership_context_ref: "seoul_clinic_ward_a",
            weight_parts_per_10_000: 5_000,
        },
        MembershipShareInput {
            membership_context_ref: "seoul_clinic_ward_a",
            weight_parts_per_10_000: 5_000,
        },
    ];
    let duplicate = observations
        .ingest(evening_mood_input(
            "observation_record_evening_005",
            "gyeot_obs_phone_88f6",
            &duplicates,
            evening_mood_times(),
        ))
        .expect_err("duplicate clinic context would collapse two memberships");
    assert_eq!(
        duplicate,
        LongitudinalObservationError::DuplicateMembershipContext
    );
}

#[test]
fn source_clock_skew_is_flagged_and_platform_rewind_fails_closed() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();
    let mut skewed = evening_mood_times();
    skewed.recorded_at_unix_ms = RECEIVED_AT_UNIX_MS + 60_000;
    let flagged = observations
        .ingest(evening_mood_input(
            "observation_record_skewed_001",
            "gyeot_obs_phone_clock_skew",
            &shares,
            skewed,
        ))
        .expect("keep the phone clock; do not rewrite it to receipt time");
    assert_eq!(
        flagged.clock_anomaly(),
        Some(ClockAnomaly::RecordedAfterReceived)
    );
    assert_eq!(flagged.recorded_at_unix_ms(), RECEIVED_AT_UNIX_MS + 60_000);
    assert_eq!(flagged.received_at_unix_ms(), RECEIVED_AT_UNIX_MS);

    let mut rewind = evening_mood_times();
    rewind.ingested_at_unix_ms = RECEIVED_AT_UNIX_MS - 1;
    let platform = observations
        .ingest(evening_mood_input(
            "observation_record_rewind_001",
            "gyeot_obs_phone_rewind",
            &shares,
            rewind,
        ))
        .expect_err("Commons must not ingest before it received the candidate");
    assert_eq!(
        platform,
        LongitudinalObservationError::InvalidPlatformOrdering
    );
    assert!(platform
        .to_string()
        .contains("ingest only at or after receipt"));
}

#[test]
fn zero_or_inverted_validity_and_blank_timezone_fail_closed() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();

    let mut zero = evening_mood_times();
    zero.validity_start_at_unix_ms = 0;
    assert_eq!(
        observations
            .ingest(evening_mood_input(
                "observation_record_zero",
                "gyeot_obs_phone_zero",
                &shares,
                zero,
            ))
            .expect_err("zero validity is missing observed time"),
        LongitudinalObservationError::InvalidTimestamp
    );

    let mut inverted = evening_mood_times();
    inverted.validity_end_at_unix_ms = OBSERVED_AT_UNIX_MS - 1;
    assert_eq!(
        observations
            .ingest(evening_mood_input(
                "observation_record_inverted",
                "gyeot_obs_phone_inverted",
                &shares,
                inverted,
            ))
            .expect_err("an interval cannot end before it starts"),
        LongitudinalObservationError::InvalidValidityInterval
    );

    let mut timezone = evening_mood_times();
    timezone.timezone_name = " ";
    assert_eq!(
        observations
            .ingest(evening_mood_input(
                "observation_record_tz",
                "gyeot_obs_phone_tz",
                &shares,
                timezone,
            ))
            .expect_err("UTC conversion without the civil timezone loses DST context"),
        LongitudinalObservationError::InvalidTimezone
    );

    let mut offset = evening_mood_times();
    offset.utc_offset_minutes = 19 * 60;
    assert_eq!(
        observations
            .ingest(evening_mood_input(
                "observation_record_offset",
                "gyeot_obs_phone_offset",
                &shares,
                offset,
            ))
            .expect_err("an impossible offset is not a timezone"),
        LongitudinalObservationError::InvalidUtcOffset
    );
}

#[test]
fn padded_enrollment_alias_is_rejected_and_later_ingest_stays_monotonic() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();
    let mut padded = evening_mood_input(
        "observation_record_evening_001",
        "gyeot_obs_phone_88f2",
        &shares,
        evening_mood_times(),
    );
    padded.enrollment_ref = " enrollment_seoul_clinic_2026q3 ";
    assert_eq!(
        observations
            .ingest(padded)
            .expect_err("do not trim a second clinic enrollment into the first"),
        LongitudinalObservationError::InvalidReference
    );

    observations
        .ingest(evening_mood_input(
            "observation_record_evening_001",
            "gyeot_obs_phone_88f2",
            &shares,
            evening_mood_times(),
        ))
        .expect("first evening row");

    let mut earlier_platform = evening_mood_times();
    earlier_platform.received_at_unix_ms = RECEIVED_AT_UNIX_MS + 1_000;
    earlier_platform.ingested_at_unix_ms = RECEIVED_AT_UNIX_MS + 2_000;
    let rewind = observations
        .ingest(evening_mood_input(
            "observation_record_evening_002",
            "gyeot_obs_phone_88f7",
            &shares,
            earlier_platform,
        ))
        .expect_err("a later phone observation cannot ingest earlier than the last accept");
    assert_eq!(rewind, LongitudinalObservationError::NonMonotonicIngestion);
    assert!(rewind
        .to_string()
        .contains("at or after the last accepted ingest"));
}

#[test]
fn operator_copy_names_the_next_legal_ingest_action() {
    assert!(LongitudinalObservationSet::new().is_empty());
    assert!(LongitudinalObservationSet::default().is_empty());

    let messages = [
        (
            LongitudinalObservationError::InvalidReference,
            "copy the exact opaque",
        ),
        (
            LongitudinalObservationError::InvalidTimestamp,
            "non-zero Unix time",
        ),
        (
            LongitudinalObservationError::InvalidValidityInterval,
            "validity_end_at at or after",
        ),
        (
            LongitudinalObservationError::InvalidTimezone,
            "IANA timezone",
        ),
        (
            LongitudinalObservationError::InvalidUtcOffset,
            "between -720 and 840",
        ),
        (
            LongitudinalObservationError::EmptyMembership,
            "declare every membership context",
        ),
        (
            LongitudinalObservationError::DuplicateMembershipContext,
            "declare each membership context once",
        ),
        (
            LongitudinalObservationError::InvalidMembershipWeight,
            "positive share of 10,000",
        ),
        (
            LongitudinalObservationError::MembershipWeightsDoNotSum,
            "add to 10,000",
        ),
        (
            LongitudinalObservationError::InvalidPlatformOrdering,
            "ingest only at or after receipt",
        ),
        (
            LongitudinalObservationError::NonMonotonicIngestion,
            "last accepted ingest time",
        ),
        (
            LongitudinalObservationError::IdempotencyConflict,
            "replay the first accepted observation",
        ),
    ];
    for (error, expected) in messages {
        assert!(
            error.to_string().contains(expected),
            "{error:?} should tell the operator {expected}"
        );
    }
}

#[test]
fn numeric_source_identity_and_overflowing_shares_fail_closed() {
    let shares = seoul_clinic_shares();
    let mut observations = LongitudinalObservationSet::new();
    let mut numeric = evening_mood_input(
        "observation_record_evening_001",
        "88",
        &shares,
        evening_mood_times(),
    );
    numeric.source_observation_ref = "88";
    assert_eq!(
        observations
            .ingest(numeric)
            .expect_err("a phone sequence number is not an opaque source identity"),
        LongitudinalObservationError::InvalidReference
    );

    let overflowing = [
        MembershipShareInput {
            membership_context_ref: "seoul_clinic_ward_a",
            weight_parts_per_10_000: 40_000,
        },
        MembershipShareInput {
            membership_context_ref: "evening_shift_team",
            weight_parts_per_10_000: 40_000,
        },
    ];
    assert_eq!(
        observations
            .ingest(evening_mood_input(
                "observation_record_overflow",
                "gyeot_obs_phone_overflow",
                &overflowing,
                evening_mood_times(),
            ))
            .expect_err("shares larger than 10,000 cannot hide a remainder"),
        LongitudinalObservationError::MembershipWeightsDoNotSum
    );
}
