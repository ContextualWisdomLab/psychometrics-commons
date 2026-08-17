//! `PostgreSQL` persistence for immutable normalized longitudinal observations.
//!
//! Commons persists ingestion evidence only. Gyeot remains the collection owner
//! and TEPP remains the temporal and multiple-membership analysis owner.

use crate::longitudinal_observation::{ClockAnomaly, LongitudinalObservationRecord};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const LONGITUDINAL_OBSERVATION_MIGRATION: &str =
    include_str!("../migrations/0031_longitudinal_observation.sql");

/// Outcome of persisting one immutable normalized observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LongitudinalObservationPersistenceDisposition {
    /// A new observation and all membership shares were inserted.
    Inserted,
    /// The exact immutable observation was already present.
    Duplicate,
}

/// Fail-closed error for longitudinal observation persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum LongitudinalObservationPersistenceError {
    /// A Rust clock or sequence cannot be represented by PostgreSQL `bigint`.
    InvalidNumericRange,
    /// An observation or source identity was replayed with different evidence.
    ConflictingReplay,
    /// Persistence requires PostgreSQL `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// PostgreSQL rejected or could not execute the operation.
    Database(postgres::Error),
}

impl Display for LongitudinalObservationPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidNumericRange => "longitudinal observation clocks or membership sequence exceed the PostgreSQL bigint range",
            Self::ConflictingReplay => "longitudinal observation identity was replayed with conflicting immutable evidence",
            Self::UnsupportedIsolationLevel => "longitudinal observation persistence requires read committed isolation",
            Self::Database(_) => "PostgreSQL longitudinal observation persistence failed",
        })
    }
}

impl Error for LongitudinalObservationPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for LongitudinalObservationPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent longitudinal-observation migration.
///
/// # Errors
///
/// Returns the PostgreSQL error when the schema cannot be installed.
pub fn apply_longitudinal_observation_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(LONGITUDINAL_OBSERVATION_MIGRATION)
}

/// Persist one normalized observation and its complete membership vector.
///
/// Exact replay is idempotent. Reusing either the Commons observation identity
/// or the `(enrollment, source system, source observation)` identity with any
/// different evidence fails closed. The caller owns the transaction and must use
/// `READ COMMITTED` so a concurrent winner is visible to replay classification.
///
/// # Errors
///
/// Returns [`LongitudinalObservationPersistenceError`] for unsupported isolation,
/// unrepresentable numeric values, conflicting replay, or a database failure.
pub fn persist_longitudinal_observation(
    transaction: &mut Transaction<'_>,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    require_read_committed(transaction)?;
    let validity_start = postgres_u64(record.validity_start_at_unix_ms())?;
    let validity_end = postgres_u64(record.validity_end_at_unix_ms())?;
    let recorded_at = postgres_u64(record.recorded_at_unix_ms())?;
    let received_at = postgres_u64(record.received_at_unix_ms())?;
    let ingested_at = postgres_u64(record.ingested_at_unix_ms())?;
    let anomaly_code = clock_anomaly_code(record.clock_anomaly());

    let inserted = transaction.execute(
        "INSERT INTO longitudinal_observation (\
             observation_record_ref, enrollment_ref, source_system_ref, source_observation_ref, \
             construct_ref, measure_ref, validity_start_at_unix_ms, validity_end_at_unix_ms, \
             recorded_at_unix_ms, received_at_unix_ms, ingested_at_unix_ms, timezone_name, \
             utc_offset_minutes, clock_anomaly_code\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT DO NOTHING",
        &[
            &record.observation_record_ref(), &record.enrollment_ref(), &record.source_system_ref(),
            &record.source_observation_ref(), &record.construct_ref(), &record.measure_ref(),
            &validity_start, &validity_end, &recorded_at, &received_at, &ingested_at,
            &record.timezone_name(), &record.utc_offset_minutes(), &anomaly_code,
        ],
    )?;
    if inserted == 0 {
        return classify_existing(transaction, record);
    }

    for (index, share) in record.membership_shares().iter().enumerate() {
        let sequence = postgres_usize(index + 1)?;
        let weight = i32::from(share.weight_parts_per_10_000());
        transaction.execute(
            "INSERT INTO longitudinal_membership_share (\
                 observation_record_ref, membership_sequence, membership_context_ref, weight_parts_per_10_000\
             ) VALUES ($1,$2,$3,$4)",
            &[&record.observation_record_ref(), &sequence, &share.membership_context_ref(), &weight],
        )?;
    }
    Ok(LongitudinalObservationPersistenceDisposition::Inserted)
}

fn classify_existing(
    transaction: &mut Transaction<'_>,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    let rows = transaction.query(
        "SELECT observation_record_ref, enrollment_ref, source_system_ref, source_observation_ref, \
                construct_ref, measure_ref, validity_start_at_unix_ms, validity_end_at_unix_ms, \
                recorded_at_unix_ms, received_at_unix_ms, ingested_at_unix_ms, timezone_name, \
                utc_offset_minutes, clock_anomaly_code \
         FROM longitudinal_observation \
         WHERE observation_record_ref = $1 OR (enrollment_ref = $2 AND source_system_ref = $3 AND source_observation_ref = $4)",
        &[&record.observation_record_ref(), &record.enrollment_ref(), &record.source_system_ref(), &record.source_observation_ref()],
    )?;
    if rows.len() != 1 {
        return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
    }
    let row = &rows[0];
    let anomaly_code = clock_anomaly_code(record.clock_anomaly()).map(str::to_owned);
    let exact_header = row.get::<_, String>(0) == record.observation_record_ref()
        && row.get::<_, String>(1) == record.enrollment_ref()
        && row.get::<_, String>(2) == record.source_system_ref()
        && row.get::<_, String>(3) == record.source_observation_ref()
        && row.get::<_, String>(4) == record.construct_ref()
        && row.get::<_, String>(5) == record.measure_ref()
        && row.get::<_, i64>(6) == postgres_u64(record.validity_start_at_unix_ms())?
        && row.get::<_, i64>(7) == postgres_u64(record.validity_end_at_unix_ms())?
        && row.get::<_, i64>(8) == postgres_u64(record.recorded_at_unix_ms())?
        && row.get::<_, i64>(9) == postgres_u64(record.received_at_unix_ms())?
        && row.get::<_, i64>(10) == postgres_u64(record.ingested_at_unix_ms())?
        && row.get::<_, String>(11) == record.timezone_name()
        && row.get::<_, i16>(12) == record.utc_offset_minutes()
        && row.get::<_, Option<String>>(13) == anomaly_code;
    if !exact_header {
        return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
    }

    let stored = transaction.query(
        "SELECT membership_sequence, membership_context_ref, weight_parts_per_10_000 \
         FROM longitudinal_membership_share WHERE observation_record_ref = $1 ORDER BY membership_sequence",
        &[&record.observation_record_ref()],
    )?;
    if stored.len() != record.membership_shares().len() {
        return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
    }
    for (index, (row, share)) in stored.iter().zip(record.membership_shares()).enumerate() {
        if row.get::<_, i64>(0) != postgres_usize(index + 1)?
            || row.get::<_, String>(1) != share.membership_context_ref()
            || row.get::<_, i32>(2) != i32::from(share.weight_parts_per_10_000())
        {
            return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
        }
    }
    Ok(LongitudinalObservationPersistenceDisposition::Duplicate)
}

fn clock_anomaly_code(anomaly: Option<ClockAnomaly>) -> Option<&'static str> {
    anomaly.map(|value| match value {
        ClockAnomaly::RecordedAfterReceived => "recorded_after_received",
    })
}

fn postgres_u64(value: u64) -> Result<i64, LongitudinalObservationPersistenceError> {
    i64::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::InvalidNumericRange)
}

fn postgres_usize(value: usize) -> Result<i64, LongitudinalObservationPersistenceError> {
    i64::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::InvalidNumericRange)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), LongitudinalObservationPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(LongitudinalObservationPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod numeric_guard_tests {
    use super::{postgres_u64, postgres_usize, LongitudinalObservationPersistenceError};
    use std::error::Error;

    #[test]
    fn numeric_conversion_and_error_sources_fail_closed() {
        assert_eq!(postgres_u64(7).unwrap(), 7);
        assert!(matches!(postgres_u64(u64::MAX), Err(LongitudinalObservationPersistenceError::InvalidNumericRange)));
        assert_eq!(postgres_usize(1).unwrap(), 1);
        if usize::BITS > 63 {
            assert!(matches!(postgres_usize(usize::MAX), Err(LongitudinalObservationPersistenceError::InvalidNumericRange)));
        }
        let error = LongitudinalObservationPersistenceError::ConflictingReplay;
        assert!(Error::source(&error).is_none());
        assert!(error.to_string().contains("conflicting"));
    }
}
