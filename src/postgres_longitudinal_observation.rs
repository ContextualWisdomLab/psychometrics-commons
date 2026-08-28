//! `PostgreSQL` persistence for immutable normalized longitudinal observations.
//!
//! Commons persists tenant-bound ingestion evidence only. Gyeot is the collection
//! service that captures longitudinal observations, while TEPP is the analysis
//! service responsible for temporal and multiple-membership models. This module
//! stores the normalized evidence without taking over either service's responsibility.

use crate::longitudinal_observation::{
    ClockAnomaly, LongitudinalObservationInput, LongitudinalObservationRecord,
    LongitudinalObservationSet, MembershipShareInput, ObservationTimeInput,
};
use crate::reference::normalized_reference;
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
    /// A tenant or observation-record reference was blank, numeric-like, padded, or noncanonical.
    InvalidReference,
    /// A Rust clock or sequence cannot be represented by `PostgreSQL` `bigint`.
    InvalidNumericRange,
    /// A distinct source observation attempted to reuse an existing Commons record identity.
    ObservationIdentityConflict,
    /// An observation or tenant-scoped source identity was replayed with different evidence.
    ConflictingReplay,
    /// Persisted rows cannot be reconstructed into one valid immutable domain record.
    CorruptHistory,
    /// Persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the operation.
    Database(postgres::Error),
}

impl Display for LongitudinalObservationPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "longitudinal observation tenant and record references must use their exact opaque form"
            }
            Self::InvalidNumericRange => {
                "longitudinal observation clocks or membership sequence exceed the PostgreSQL bigint range"
            }
            Self::ObservationIdentityConflict => {
                "a distinct longitudinal source observation cannot reuse an existing Commons observation record identity"
            }
            Self::ConflictingReplay => {
                "longitudinal observation identity was replayed with conflicting immutable evidence"
            }
            Self::CorruptHistory => {
                "stored longitudinal observation evidence cannot be reconstructed safely"
            }
            Self::UnsupportedIsolationLevel => {
                "longitudinal observation persistence requires read committed isolation"
            }
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
/// The migration uses unqualified database object names. Callers must set the
/// intended `PostgreSQL` `search_path` before applying it and use the same schema
/// context for related persistence queries.
///
/// # Errors
///
/// Returns the `PostgreSQL` error when the schema cannot be installed.
pub fn apply_longitudinal_observation_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(LONGITUDINAL_OBSERVATION_MIGRATION)
}

/// Persist one tenant-bound normalized observation and its complete membership vector.
///
/// Exact replay is idempotent. Reusing either the global Commons observation
/// identity or the tenant-scoped `(enrollment, source system, source observation)`
/// identity with different evidence fails closed. A distinct source that reuses an
/// existing Commons record identity is classified separately as an observation-identity
/// conflict. The same source tuple may exist in a different tenant only under a different
/// observation-record identity. The caller owns the transaction and must use
/// `READ COMMITTED` so a concurrent winner is visible to replay classification.
///
/// # Errors
///
/// Returns [`LongitudinalObservationPersistenceError`] for a noncanonical tenant,
/// unsupported isolation, unrepresentable numeric values, observation-record identity
/// collision, conflicting replay, or a database failure.
pub fn persist_longitudinal_observation(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    let tenant_ref = required_reference(tenant_ref)?;
    require_read_committed(transaction)?;
    let validity_start = postgres_u64(record.validity_start_at_unix_ms())?;
    let validity_end = postgres_u64(record.validity_end_at_unix_ms())?;
    let recorded_at = postgres_u64(record.recorded_at_unix_ms())?;
    let received_at = postgres_u64(record.received_at_unix_ms())?;
    let ingested_at = postgres_u64(record.ingested_at_unix_ms())?;
    let anomaly_code = clock_anomaly_code(record.clock_anomaly());

    let inserted = transaction.execute(
        "INSERT INTO longitudinal_observation (\
             observation_record_ref, tenant_ref, enrollment_ref, source_system_ref, \
             source_observation_ref, construct_ref, measure_ref, validity_start_at_unix_ms, \
             validity_end_at_unix_ms, recorded_at_unix_ms, received_at_unix_ms, \
             ingested_at_unix_ms, timezone_name, utc_offset_minutes, clock_anomaly_code\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) \
         ON CONFLICT DO NOTHING",
        &[
            &record.observation_record_ref(),
            &tenant_ref,
            &record.enrollment_ref(),
            &record.source_system_ref(),
            &record.source_observation_ref(),
            &record.construct_ref(),
            &record.measure_ref(),
            &validity_start,
            &validity_end,
            &recorded_at,
            &received_at,
            &ingested_at,
            &record.timezone_name(),
            &record.utc_offset_minutes(),
            &anomaly_code,
        ],
    )?;
    if inserted == 0 {
        return classify_existing(transaction, tenant_ref, record);
    }

    for (index, share) in record.membership_shares().iter().enumerate() {
        let sequence = postgres_usize(index + 1)?;
        let weight = i32::from(share.weight_parts_per_10_000());
        transaction.execute(
            "INSERT INTO longitudinal_membership_share (\
                 observation_record_ref, membership_sequence, membership_context_ref, \
                 weight_parts_per_10_000\
             ) VALUES ($1,$2,$3,$4)",
            &[
                &record.observation_record_ref(),
                &sequence,
                &share.membership_context_ref(),
                &weight,
            ],
        )?;
    }
    Ok(LongitudinalObservationPersistenceDisposition::Inserted)
}

/// Load one immutable observation after process restart within a tenant boundary.
///
/// The loader rebuilds the public domain record through the same validation path
/// used for fresh ingestion. Tenant mismatch and a missing record return `None`;
/// incomplete, non-contiguous, numerically invalid, or internally inconsistent
/// stored evidence fails closed as [`LongitudinalObservationPersistenceError::CorruptHistory`].
///
/// # Errors
///
/// Returns [`LongitudinalObservationPersistenceError`] for noncanonical references,
/// corrupt stored evidence, or a database failure.
pub fn load_longitudinal_observation(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    observation_record_ref: &str,
) -> Result<Option<LongitudinalObservationRecord>, LongitudinalObservationPersistenceError> {
    let tenant_ref = required_reference(tenant_ref)?;
    let observation_record_ref = required_reference(observation_record_ref)?;
    let row = transaction.query_opt(
        "SELECT enrollment_ref, source_system_ref, source_observation_ref, construct_ref, \
                measure_ref, validity_start_at_unix_ms, validity_end_at_unix_ms, \
                recorded_at_unix_ms, received_at_unix_ms, ingested_at_unix_ms, \
                timezone_name, utc_offset_minutes, clock_anomaly_code \
         FROM longitudinal_observation \
         WHERE tenant_ref = $1 AND observation_record_ref = $2",
        &[&tenant_ref, &observation_record_ref],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };

    let membership_rows = transaction.query(
        "SELECT membership_sequence, membership_context_ref, weight_parts_per_10_000 \
         FROM longitudinal_membership_share \
         WHERE observation_record_ref = $1 ORDER BY membership_sequence",
        &[&observation_record_ref],
    )?;
    let mut stored_memberships = Vec::with_capacity(membership_rows.len());
    for (index, membership_row) in membership_rows.iter().enumerate() {
        require_membership_sequence(membership_row.get(0), postgres_usize(index + 1)?)?;
        let membership_context_ref: String = membership_row.get(1);
        let weight_parts_per_10_000 = database_u16(membership_row.get(2))?;
        stored_memberships.push((membership_context_ref, weight_parts_per_10_000));
    }
    let membership_inputs = stored_memberships
        .iter()
        .map(
            |(membership_context_ref, weight_parts_per_10_000)| MembershipShareInput {
                membership_context_ref,
                weight_parts_per_10_000: *weight_parts_per_10_000,
            },
        )
        .collect::<Vec<_>>();

    let enrollment_ref: String = row.get(0);
    let source_system_ref: String = row.get(1);
    let source_observation_ref: String = row.get(2);
    let construct_ref: String = row.get(3);
    let measure_ref: String = row.get(4);
    let validity_start_at_unix_ms = database_u64(row.get(5))?;
    let validity_end_at_unix_ms = database_u64(row.get(6))?;
    let recorded_at_unix_ms = database_u64(row.get(7))?;
    let received_at_unix_ms = database_u64(row.get(8))?;
    let ingested_at_unix_ms = database_u64(row.get(9))?;
    let timezone_name: String = row.get(10);
    let utc_offset_minutes: i16 = row.get(11);
    let stored_anomaly_code: Option<String> = row.get(12);

    let record = LongitudinalObservationSet::new()
        .ingest(LongitudinalObservationInput {
            observation_record_ref,
            enrollment_ref: &enrollment_ref,
            source_system_ref: &source_system_ref,
            source_observation_ref: &source_observation_ref,
            construct_ref: &construct_ref,
            measure_ref: &measure_ref,
            membership_shares: &membership_inputs,
            time: ObservationTimeInput {
                validity_start_at_unix_ms,
                validity_end_at_unix_ms,
                recorded_at_unix_ms,
                received_at_unix_ms,
                ingested_at_unix_ms,
                timezone_name: &timezone_name,
                utc_offset_minutes,
            },
        })
        .map_err(|_| LongitudinalObservationPersistenceError::CorruptHistory)?;
    require_clock_anomaly_code(stored_anomaly_code.as_deref(), record.clock_anomaly())?;
    Ok(Some(record))
}

fn classify_existing(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    record: &LongitudinalObservationRecord,
) -> Result<LongitudinalObservationPersistenceDisposition, LongitudinalObservationPersistenceError>
{
    let rows = transaction.query(
        "SELECT observation_record_ref, tenant_ref, enrollment_ref, source_system_ref, \
                source_observation_ref, construct_ref, measure_ref, validity_start_at_unix_ms, \
                validity_end_at_unix_ms, recorded_at_unix_ms, received_at_unix_ms, \
                ingested_at_unix_ms, timezone_name, utc_offset_minutes, clock_anomaly_code \
         FROM longitudinal_observation \
         WHERE observation_record_ref = $1 \
            OR (tenant_ref = $2 AND enrollment_ref = $3 AND source_system_ref = $4 \
                AND source_observation_ref = $5)",
        &[
            &record.observation_record_ref(),
            &tenant_ref,
            &record.enrollment_ref(),
            &record.source_system_ref(),
            &record.source_observation_ref(),
        ],
    )?;
    let record_identity_collision = rows.iter().any(|row| {
        row.get::<_, String>(0) == record.observation_record_ref()
            && (row.get::<_, String>(1) != tenant_ref
                || row.get::<_, String>(2) != record.enrollment_ref()
                || row.get::<_, String>(3) != record.source_system_ref()
                || row.get::<_, String>(4) != record.source_observation_ref())
    });
    if record_identity_collision {
        return Err(LongitudinalObservationPersistenceError::ObservationIdentityConflict);
    }
    if rows.len() != 1 {
        return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
    }
    let row = &rows[0];
    let anomaly_code = clock_anomaly_code(record.clock_anomaly()).map(str::to_owned);
    let exact_header = row.get::<_, String>(0) == record.observation_record_ref()
        && row.get::<_, String>(1) == tenant_ref
        && row.get::<_, String>(2) == record.enrollment_ref()
        && row.get::<_, String>(3) == record.source_system_ref()
        && row.get::<_, String>(4) == record.source_observation_ref()
        && row.get::<_, String>(5) == record.construct_ref()
        && row.get::<_, String>(6) == record.measure_ref()
        && row.get::<_, i64>(7) == postgres_u64(record.validity_start_at_unix_ms())?
        && row.get::<_, i64>(8) == postgres_u64(record.validity_end_at_unix_ms())?
        && row.get::<_, i64>(9) == postgres_u64(record.recorded_at_unix_ms())?
        && row.get::<_, i64>(10) == postgres_u64(record.received_at_unix_ms())?
        && row.get::<_, i64>(11) == postgres_u64(record.ingested_at_unix_ms())?
        && row.get::<_, String>(12) == record.timezone_name()
        && row.get::<_, i16>(13) == record.utc_offset_minutes()
        && row.get::<_, Option<String>>(14) == anomaly_code;
    if !exact_header {
        return Err(LongitudinalObservationPersistenceError::ConflictingReplay);
    }

    let stored = transaction.query(
        "SELECT membership_sequence, membership_context_ref, weight_parts_per_10_000 \
         FROM longitudinal_membership_share \
         WHERE observation_record_ref = $1 ORDER BY membership_sequence",
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

fn required_reference(reference: &str) -> Result<&str, LongitudinalObservationPersistenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(LongitudinalObservationPersistenceError::InvalidReference),
    }
}

fn clock_anomaly_code(anomaly: Option<ClockAnomaly>) -> Option<&'static str> {
    anomaly.map(|value| match value {
        ClockAnomaly::RecordedAfterReceived => "recorded_after_received",
    })
}

fn require_clock_anomaly_code(
    stored_code: Option<&str>,
    anomaly: Option<ClockAnomaly>,
) -> Result<(), LongitudinalObservationPersistenceError> {
    if stored_code == clock_anomaly_code(anomaly) {
        Ok(())
    } else {
        Err(LongitudinalObservationPersistenceError::CorruptHistory)
    }
}

fn postgres_u64(value: u64) -> Result<i64, LongitudinalObservationPersistenceError> {
    i64::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::InvalidNumericRange)
}

fn postgres_usize(value: usize) -> Result<i64, LongitudinalObservationPersistenceError> {
    i64::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::InvalidNumericRange)
}

fn database_u64(value: i64) -> Result<u64, LongitudinalObservationPersistenceError> {
    u64::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::CorruptHistory)
}

fn database_u16(value: i32) -> Result<u16, LongitudinalObservationPersistenceError> {
    u16::try_from(value).map_err(|_| LongitudinalObservationPersistenceError::CorruptHistory)
}

fn require_membership_sequence(
    stored_sequence: i64,
    expected_sequence: i64,
) -> Result<(), LongitudinalObservationPersistenceError> {
    if stored_sequence == expected_sequence {
        Ok(())
    } else {
        Err(LongitudinalObservationPersistenceError::CorruptHistory)
    }
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
    use super::{
        database_u16, database_u64, postgres_u64, postgres_usize, require_clock_anomaly_code,
        require_membership_sequence, required_reference, LongitudinalObservationPersistenceError,
    };
    use crate::longitudinal_observation::ClockAnomaly;
    use std::error::Error;

    #[test]
    fn reference_numeric_conversion_and_error_sources_fail_closed() {
        assert_eq!(
            required_reference("tenant_clinic_seoul").unwrap(),
            "tenant_clinic_seoul"
        );
        assert!(matches!(
            required_reference(" tenant_clinic_seoul "),
            Err(LongitudinalObservationPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(LongitudinalObservationPersistenceError::InvalidReference)
        ));
        assert_eq!(postgres_u64(7).unwrap(), 7);
        assert!(matches!(
            postgres_u64(u64::MAX),
            Err(LongitudinalObservationPersistenceError::InvalidNumericRange)
        ));
        assert_eq!(postgres_usize(1).unwrap(), 1);
        #[cfg(target_pointer_width = "64")]
        assert!(matches!(
            postgres_usize(usize::MAX),
            Err(LongitudinalObservationPersistenceError::InvalidNumericRange)
        ));
        #[cfg(not(target_pointer_width = "64"))]
        assert_eq!(
            postgres_usize(usize::MAX).unwrap(),
            i64::try_from(usize::MAX).unwrap()
        );
        assert_eq!(database_u64(7).unwrap(), 7);
        assert!(matches!(
            database_u64(-1),
            Err(LongitudinalObservationPersistenceError::CorruptHistory)
        ));
        assert_eq!(database_u16(10_000).unwrap(), 10_000);
        assert!(matches!(
            database_u16(-1),
            Err(LongitudinalObservationPersistenceError::CorruptHistory)
        ));
        assert!(require_membership_sequence(1, 1).is_ok());
        assert!(matches!(
            require_membership_sequence(2, 1),
            Err(LongitudinalObservationPersistenceError::CorruptHistory)
        ));
        assert!(require_clock_anomaly_code(None, None).is_ok());
        assert!(require_clock_anomaly_code(
            Some("recorded_after_received"),
            Some(ClockAnomaly::RecordedAfterReceived)
        )
        .is_ok());
        assert!(matches!(
            require_clock_anomaly_code(None, Some(ClockAnomaly::RecordedAfterReceived)),
            Err(LongitudinalObservationPersistenceError::CorruptHistory)
        ));
        let identity_conflict = LongitudinalObservationPersistenceError::ObservationIdentityConflict;
        assert!(Error::source(&identity_conflict).is_none());
        assert!(identity_conflict.to_string().contains("record identity"));
        let error = LongitudinalObservationPersistenceError::ConflictingReplay;
        assert!(Error::source(&error).is_none());
        assert!(error.to_string().contains("conflicting"));
        let corrupt = LongitudinalObservationPersistenceError::CorruptHistory;
        assert!(corrupt.to_string().contains("reconstructed"));
    }
}
