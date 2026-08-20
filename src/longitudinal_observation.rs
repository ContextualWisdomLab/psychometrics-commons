//! Normalized longitudinal observation identity, clocks, and membership shares.
//!
//! Psychometrics Commons owns consented ingestion evidence. Gyeot still collects
//! the mobile observation and TEPP still owns temporal and multiple-membership
//! kernels. This module keeps validity, source-recorded, platform-received, and
//! durable-ingest times distinct, and it keeps every declared membership weight
//! visible so a later analysis cannot flatten the row into one primary group.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

const MEMBERSHIP_WEIGHT_TOTAL: u16 = 10_000;
const MIN_UTC_OFFSET_MINUTES: i16 = -12 * 60;
const MAX_UTC_OFFSET_MINUTES: i16 = 14 * 60;

/// One explicit share of a higher-level context for a single observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MembershipShareInput<'a> {
    /// Opaque context the observation belongs to, such as a clinic ward or shift team.
    pub membership_context_ref: &'a str,
    /// Integer share of 10,000. Every declared context must have a positive share
    /// and the shares on one observation must sum to 10,000.
    pub weight_parts_per_10_000: u16,
}

/// Stored explicit membership share after validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MembershipShare {
    membership_context_ref: String,
    weight_parts_per_10_000: u16,
}

impl MembershipShare {
    /// Return the opaque membership context.
    #[must_use]
    pub fn membership_context_ref(&self) -> &str {
        &self.membership_context_ref
    }

    /// Return the integer share of 10,000 for this context.
    #[must_use]
    pub const fn weight_parts_per_10_000(&self) -> u16 {
        self.weight_parts_per_10_000
    }
}

/// Four distinct clocks plus the civil timezone that produced them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationTimeInput<'a> {
    /// Validity-interval start. A point observation uses the observed instant.
    pub validity_start_at_unix_ms: u64,
    /// Validity-interval end. A point observation repeats the observed instant.
    pub validity_end_at_unix_ms: u64,
    /// When the collection client stored the observation.
    pub recorded_at_unix_ms: u64,
    /// When Commons first received the candidate at its trust boundary.
    pub received_at_unix_ms: u64,
    /// When Commons durably accepted the normalized row.
    pub ingested_at_unix_ms: u64,
    /// IANA timezone name that preserves civil time and daylight-saving context.
    pub timezone_name: &'a str,
    /// Offset from UTC, in minutes, that was in force at the observed instant.
    pub utc_offset_minutes: i16,
}

/// Borrowed evidence required to ingest one Gyeot observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LongitudinalObservationInput<'a> {
    /// Opaque Commons observation-record identity.
    pub observation_record_ref: &'a str,
    /// Enrollment that authorized collection. This module does not own enrollment state.
    pub enrollment_ref: &'a str,
    /// Collection system that minted the source observation, usually Gyeot.
    pub source_system_ref: &'a str,
    /// Stable source observation identity used for idempotent sync.
    pub source_observation_ref: &'a str,
    /// Construct the observation measures.
    pub construct_ref: &'a str,
    /// Exact measure version used at collection.
    pub measure_ref: &'a str,
    /// Explicit multiple-membership shares. An empty list is rejected.
    pub membership_shares: &'a [MembershipShareInput<'a>],
    /// Distinct source and platform clocks for this observation.
    pub time: ObservationTimeInput<'a>,
}

/// Source-clock anomaly that was retained instead of rewritten.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ClockAnomaly {
    /// The client stored the observation after Commons received it.
    RecordedAfterReceived,
}

/// Immutable normalized observation accepted by Commons.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongitudinalObservationRecord {
    observation_record_ref: String,
    enrollment_ref: String,
    source_system_ref: String,
    source_observation_ref: String,
    construct_ref: String,
    measure_ref: String,
    membership_shares: Vec<MembershipShare>,
    validity_start_at_unix_ms: u64,
    validity_end_at_unix_ms: u64,
    recorded_at_unix_ms: u64,
    received_at_unix_ms: u64,
    ingested_at_unix_ms: u64,
    timezone_name: String,
    utc_offset_minutes: i16,
    clock_anomaly: Option<ClockAnomaly>,
}

impl LongitudinalObservationRecord {
    /// Return the opaque observation-record identity.
    #[must_use]
    pub fn observation_record_ref(&self) -> &str {
        &self.observation_record_ref
    }

    /// Return the enrollment that authorized this observation.
    #[must_use]
    pub fn enrollment_ref(&self) -> &str {
        &self.enrollment_ref
    }

    /// Return the collection-system identity.
    #[must_use]
    pub fn source_system_ref(&self) -> &str {
        &self.source_system_ref
    }

    /// Return the stable source observation identity.
    #[must_use]
    pub fn source_observation_ref(&self) -> &str {
        &self.source_observation_ref
    }

    /// Return the construct measured by this observation.
    #[must_use]
    pub fn construct_ref(&self) -> &str {
        &self.construct_ref
    }

    /// Return the exact measure version.
    #[must_use]
    pub fn measure_ref(&self) -> &str {
        &self.measure_ref
    }

    /// Return every explicit membership share in declaration order.
    #[must_use]
    pub fn membership_shares(&self) -> &[MembershipShare] {
        &self.membership_shares
    }

    /// Return the validity-interval start in Unix milliseconds.
    #[must_use]
    pub const fn validity_start_at_unix_ms(&self) -> u64 {
        self.validity_start_at_unix_ms
    }

    /// Return the validity-interval end in Unix milliseconds.
    #[must_use]
    pub const fn validity_end_at_unix_ms(&self) -> u64 {
        self.validity_end_at_unix_ms
    }

    /// Return when the collection client stored the observation.
    #[must_use]
    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }

    /// Return when Commons received the candidate.
    #[must_use]
    pub const fn received_at_unix_ms(&self) -> u64 {
        self.received_at_unix_ms
    }

    /// Return when Commons durably accepted the row.
    #[must_use]
    pub const fn ingested_at_unix_ms(&self) -> u64 {
        self.ingested_at_unix_ms
    }

    /// Return the IANA timezone name retained with the clocks.
    #[must_use]
    pub fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    /// Return the UTC offset, in minutes, retained with the observed instant.
    #[must_use]
    pub const fn utc_offset_minutes(&self) -> i16 {
        self.utc_offset_minutes
    }

    /// Return a flagged source-clock anomaly, if the source order was untrusted.
    #[must_use]
    pub const fn clock_anomaly(&self) -> Option<ClockAnomaly> {
        self.clock_anomaly
    }
}

/// Fail-closed error while ingesting one longitudinal observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LongitudinalObservationError {
    /// A required reference was blank, numeric-like, or whitespace-padded.
    InvalidReference,
    /// A required timestamp was zero.
    InvalidTimestamp,
    /// The validity interval ended before it started.
    InvalidValidityInterval,
    /// The civil timezone name was blank or numeric-like.
    InvalidTimezone,
    /// The UTC offset is outside the civil range from UTC−12 to UTC+14.
    InvalidUtcOffset,
    /// The observation declared no membership context.
    EmptyMembership,
    /// The same membership context appeared more than once.
    DuplicateMembershipContext,
    /// A membership share was zero.
    InvalidMembershipWeight,
    /// Membership shares did not sum to 10,000.
    MembershipWeightsDoNotSum,
    /// Durable ingest happened before platform receipt.
    InvalidPlatformOrdering,
    /// A later observation tried to ingest earlier than the last accepted row.
    NonMonotonicIngestion,
    /// The same source observation was replayed with different evidence.
    IdempotencyConflict,
    /// A distinct source observation tried to reuse an existing Commons record identity.
    ObservationIdentityConflict,
}

impl Display for LongitudinalObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "copy the exact opaque enrollment, source, construct, measure, or observation reference; do not pad or number it"
            }
            Self::InvalidTimestamp => {
                "send every validity, recorded, received, and ingested clock as a non-zero Unix time"
            }
            Self::InvalidValidityInterval => {
                "set validity_end_at at or after validity_start_at for this observation"
            }
            Self::InvalidTimezone => {
                "send the IANA timezone that produced the civil observation time, such as Asia/Seoul"
            }
            Self::InvalidUtcOffset => {
                "send the UTC offset in minutes between -720 and 840 that was in force at the observed instant"
            }
            Self::EmptyMembership => {
                "declare every membership context and its share of 10,000; do not ingest an observation with no group"
            }
            Self::DuplicateMembershipContext => {
                "declare each membership context once so two clinic or team shares are not collapsed"
            }
            Self::InvalidMembershipWeight => {
                "give every declared membership a positive share of 10,000"
            }
            Self::MembershipWeightsDoNotSum => {
                "make the membership shares add to 10,000 so no hidden primary remainder is invented"
            }
            Self::InvalidPlatformOrdering => {
                "ingest only at or after receipt; do not back-date Commons acceptance before the trust boundary"
            }
            Self::NonMonotonicIngestion => {
                "ingest the next observation at or after the last accepted ingest time"
            }
            Self::IdempotencyConflict => {
                "replay the first accepted observation for this source identity; do not last-write-win a new clock"
            }
            Self::ObservationIdentityConflict => {
                "mint a new Commons observation-record identity for a distinct source observation; do not reuse an existing record identity"
            }
        })
    }
}

impl Error for LongitudinalObservationError {}

/// In-memory set that accepts observations with source-identity idempotency.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LongitudinalObservationSet {
    records: Vec<LongitudinalObservationRecord>,
}

impl LongitudinalObservationSet {
    /// Create an empty observation set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Return how many distinct source observations were accepted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Return whether no observation has been accepted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Accept one observation or return the exact prior row for a matching source identity.
    ///
    /// Source identity is `(enrollment_ref, source_system_ref, source_observation_ref)`.
    /// An exact retry is a no-op. A retry with different clocks, membership, or
    /// construct evidence fails closed. A distinct source observation must also
    /// receive a distinct Commons `observation_record_ref`; a reused record identity
    /// fails closed instead of aliasing two immutable observations. Source clocks
    /// that arrive after receipt are flagged and kept. Platform ingest cannot precede
    /// receipt or the previous accepted ingest time.
    ///
    /// # Errors
    ///
    /// Returns [`LongitudinalObservationError`] when a reference, clock, timezone,
    /// membership share, platform order, source-identity replay, or Commons record
    /// identity reuse is invalid.
    pub fn ingest(
        &mut self,
        input: LongitudinalObservationInput<'_>,
    ) -> Result<LongitudinalObservationRecord, LongitudinalObservationError> {
        let candidate = validate_observation(input)?;
        if let Some(existing) = self.records.iter().find(|record| {
            record.enrollment_ref == candidate.enrollment_ref
                && record.source_system_ref == candidate.source_system_ref
                && record.source_observation_ref == candidate.source_observation_ref
        }) {
            if existing == &candidate {
                return Ok(existing.clone());
            }
            return Err(LongitudinalObservationError::IdempotencyConflict);
        }
        if self
            .records
            .iter()
            .any(|record| record.observation_record_ref == candidate.observation_record_ref)
        {
            return Err(LongitudinalObservationError::ObservationIdentityConflict);
        }
        if self
            .records
            .last()
            .is_some_and(|record| candidate.ingested_at_unix_ms < record.ingested_at_unix_ms)
        {
            return Err(LongitudinalObservationError::NonMonotonicIngestion);
        }
        self.records.push(candidate.clone());
        Ok(candidate)
    }
}

fn validate_observation(
    input: LongitudinalObservationInput<'_>,
) -> Result<LongitudinalObservationRecord, LongitudinalObservationError> {
    let observation_record_ref = required_reference(input.observation_record_ref)?;
    let enrollment_ref = required_reference(input.enrollment_ref)?;
    let source_system_ref = required_reference(input.source_system_ref)?;
    let source_observation_ref = required_reference(input.source_observation_ref)?;
    let construct_ref = required_reference(input.construct_ref)?;
    let measure_ref = required_reference(input.measure_ref)?;
    let timezone_name = required_timezone(input.time.timezone_name)?;
    require_timestamp(input.time.validity_start_at_unix_ms)?;
    require_timestamp(input.time.validity_end_at_unix_ms)?;
    require_timestamp(input.time.recorded_at_unix_ms)?;
    require_timestamp(input.time.received_at_unix_ms)?;
    require_timestamp(input.time.ingested_at_unix_ms)?;
    if input.time.validity_end_at_unix_ms < input.time.validity_start_at_unix_ms {
        return Err(LongitudinalObservationError::InvalidValidityInterval);
    }
    if !(MIN_UTC_OFFSET_MINUTES..=MAX_UTC_OFFSET_MINUTES).contains(&input.time.utc_offset_minutes) {
        return Err(LongitudinalObservationError::InvalidUtcOffset);
    }
    if input.time.ingested_at_unix_ms < input.time.received_at_unix_ms {
        return Err(LongitudinalObservationError::InvalidPlatformOrdering);
    }
    let membership_shares = unique_memberships(input.membership_shares)?;
    let clock_anomaly = (input.time.recorded_at_unix_ms > input.time.received_at_unix_ms)
        .then_some(ClockAnomaly::RecordedAfterReceived);

    Ok(LongitudinalObservationRecord {
        observation_record_ref: observation_record_ref.to_owned(),
        enrollment_ref: enrollment_ref.to_owned(),
        source_system_ref: source_system_ref.to_owned(),
        source_observation_ref: source_observation_ref.to_owned(),
        construct_ref: construct_ref.to_owned(),
        measure_ref: measure_ref.to_owned(),
        membership_shares,
        validity_start_at_unix_ms: input.time.validity_start_at_unix_ms,
        validity_end_at_unix_ms: input.time.validity_end_at_unix_ms,
        recorded_at_unix_ms: input.time.recorded_at_unix_ms,
        received_at_unix_ms: input.time.received_at_unix_ms,
        ingested_at_unix_ms: input.time.ingested_at_unix_ms,
        timezone_name: timezone_name.to_owned(),
        utc_offset_minutes: input.time.utc_offset_minutes,
        clock_anomaly,
    })
}

fn unique_memberships(
    shares: &[MembershipShareInput<'_>],
) -> Result<Vec<MembershipShare>, LongitudinalObservationError> {
    if shares.is_empty() {
        return Err(LongitudinalObservationError::EmptyMembership);
    }

    let mut accepted = Vec::with_capacity(shares.len());
    let mut total = 0_u16;
    for share in shares {
        let membership_context_ref = required_reference(share.membership_context_ref)?;
        if share.weight_parts_per_10_000 == 0 {
            return Err(LongitudinalObservationError::InvalidMembershipWeight);
        }
        if accepted.iter().any(|existing: &MembershipShare| {
            existing.membership_context_ref == membership_context_ref
        }) {
            return Err(LongitudinalObservationError::DuplicateMembershipContext);
        }
        total = total
            .checked_add(share.weight_parts_per_10_000)
            .ok_or(LongitudinalObservationError::MembershipWeightsDoNotSum)?;
        accepted.push(MembershipShare {
            membership_context_ref: membership_context_ref.to_owned(),
            weight_parts_per_10_000: share.weight_parts_per_10_000,
        });
    }
    if total != MEMBERSHIP_WEIGHT_TOTAL {
        return Err(LongitudinalObservationError::MembershipWeightsDoNotSum);
    }
    Ok(accepted)
}

fn required_reference(reference: &str) -> Result<&str, LongitudinalObservationError> {
    let canonical =
        normalized_reference(reference).ok_or(LongitudinalObservationError::InvalidReference)?;
    if canonical != reference {
        return Err(LongitudinalObservationError::InvalidReference);
    }
    Ok(canonical)
}

fn required_timezone(timezone_name: &str) -> Result<&str, LongitudinalObservationError> {
    required_reference(timezone_name).map_err(|_| LongitudinalObservationError::InvalidTimezone)
}

fn require_timestamp(timestamp: u64) -> Result<(), LongitudinalObservationError> {
    if timestamp == 0 {
        Err(LongitudinalObservationError::InvalidTimestamp)
    } else {
        Ok(())
    }
}
