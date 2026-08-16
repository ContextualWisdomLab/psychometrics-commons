//! `PostgreSQL` operational-store compatibility and write-readiness evidence.
//!
//! The runtime's initial persistence contract supports upstream `PostgreSQL` 18.x.
//! This module probes the caller-owned connection and classifies whether the database
//! can safely accept product-owned state changes. It does not own credentials,
//! connection pooling, migrations, backup, or recovery.

use crate::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, HealthContractError,
};
use postgres::GenericClient;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Initial supported `PostgreSQL` server major version from ADR-0015.
pub const SUPPORTED_POSTGRES_MAJOR: i32 = 18;

const BACKLOG_HEALTH_INDEX_MIGRATION: &str =
    include_str!("../migrations/0020_backlog_health_indexes.sql");

/// Capability identity used by operation-scoped runtime readiness.
pub const POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF: &str = "postgres_operational_store";

/// Fail-closed classification of the product operational database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PostgresRuntimeStatus {
    /// The supported `PostgreSQL` major is reachable and accepts writes.
    Ready,
    /// The `PostgreSQL` server major is outside the repository's validated support boundary.
    UnsupportedMajorVersion,
    /// The supported `PostgreSQL` server is currently read-only for this connection.
    ReadOnly,
}

/// Point-in-time `PostgreSQL` compatibility and readiness evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresRuntimeHealth {
    server_major_version: i32,
    status: PostgresRuntimeStatus,
}

impl PostgresRuntimeHealth {
    /// Return the observed `PostgreSQL` server major version.
    #[must_use]
    pub const fn server_major_version(self) -> i32 {
        self.server_major_version
    }

    /// Return the fail-closed compatibility/write-readiness classification.
    #[must_use]
    pub const fn status(self) -> PostgresRuntimeStatus {
        self.status
    }

    /// Return the generic runtime capability state represented by this database evidence.
    #[must_use]
    pub const fn capability_state(self) -> CapabilityState {
        match self.status {
            PostgresRuntimeStatus::Ready => CapabilityState::Available,
            PostgresRuntimeStatus::UnsupportedMajorVersion | PostgresRuntimeStatus::ReadOnly => {
                CapabilityState::Unavailable
            }
        }
    }

    /// Return whether this operational store may safely accept new state-changing work.
    #[must_use]
    pub const fn accepts_new_work(self) -> bool {
        matches!(self.status, PostgresRuntimeStatus::Ready)
    }

    /// Convert this database evidence into the shared operation-scoped capability contract.
    ///
    /// # Errors
    ///
    /// Returns [`HealthContractError`] only if the repository-owned constant capability
    /// reference violates the shared health contract.
    pub fn capability_health(self) -> Result<CapabilityHealth, HealthContractError> {
        CapabilityHealth::new(
            POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF,
            self.capability_state(),
            self.accepts_new_work(),
        )
    }
}

/// Operator-supplied bounds used to classify durable integration backlog evidence.
///
/// The repository deliberately provides no universal defaults. Hosted, community,
/// and enterprise operators must derive these values from their measured workload,
/// topology, alert policy, and recovery evidence rather than treating architecture
/// prose as an SLO commitment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntegrationBacklogPolicy {
    /// Largest pending outbox population still accepted for new state-changing work.
    pub max_pending_outbox_count: u64,
    /// Largest age in milliseconds of the oldest pending outbox event.
    pub max_pending_outbox_age_ms: u64,
    /// Largest quarantined outbox population still accepted under this operator policy.
    pub max_quarantined_outbox_count: u64,
    /// Largest pending-or-processing inbox-consumption population still accepted.
    pub max_active_consumption_count: u64,
    /// Largest age in milliseconds of the oldest pending-or-processing consumption.
    pub max_active_consumption_age_ms: u64,
    /// Largest quarantined inbox-consumption population still accepted.
    pub max_quarantined_consumption_count: u64,
}

/// Aggregate, content-free evidence read from the durable integration tables.
///
/// Only counts and server-authoritative event times are exposed. The probe never
/// returns assessment responses, event payloads, tenant identities, subjects, or
/// restricted research linkage values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresIntegrationBacklogEvidence {
    pending_outbox_count: u64,
    quarantined_outbox_count: u64,
    active_consumption_count: u64,
    quarantined_consumption_count: u64,
    oldest_pending_outbox_event_at_unix_ms: Option<u64>,
    oldest_active_consumption_event_at_unix_ms: Option<u64>,
}

impl PostgresIntegrationBacklogEvidence {
    /// Return the number of durable outbox events still awaiting delivery.
    #[must_use]
    pub const fn pending_outbox_count(self) -> u64 {
        self.pending_outbox_count
    }

    /// Return the number of outbox events quarantined for operator attention.
    #[must_use]
    pub const fn quarantined_outbox_count(self) -> u64 {
        self.quarantined_outbox_count
    }

    /// Return pending plus processing inbox side-effect consumptions.
    #[must_use]
    pub const fn active_consumption_count(self) -> u64 {
        self.active_consumption_count
    }

    /// Return the number of quarantined inbox side-effect consumptions.
    #[must_use]
    pub const fn quarantined_consumption_count(self) -> u64 {
        self.quarantined_consumption_count
    }

    /// Return the oldest pending outbox event time, or `None` when no event is pending.
    #[must_use]
    pub const fn oldest_pending_outbox_event_at_unix_ms(self) -> Option<u64> {
        self.oldest_pending_outbox_event_at_unix_ms
    }

    /// Return the oldest pending/processing inbox-consumption event time, if any.
    #[must_use]
    pub const fn oldest_active_consumption_event_at_unix_ms(self) -> Option<u64> {
        self.oldest_active_consumption_event_at_unix_ms
    }
}

/// Operator-supplied bounds for participant data-rights request and propagation backlogs.
///
/// Data-rights timeliness is a deployment and governance policy, not a hard-coded product
/// constant. Callers provide evidence-backed limits for their named deployment profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataRightsBacklogPolicy {
    /// Largest non-terminal participant request population still accepted.
    pub max_active_request_count: u64,
    /// Largest age in milliseconds of the oldest non-terminal request.
    pub max_active_request_age_ms: u64,
    /// Largest pending downstream propagation population still accepted.
    pub max_pending_propagation_count: u64,
    /// Largest age in milliseconds of the oldest pending downstream propagation.
    pub max_pending_propagation_age_ms: u64,
    /// Largest quarantined downstream propagation population still accepted.
    pub max_quarantined_propagation_count: u64,
}

/// Aggregate participant-rights backlog evidence without participant or tenant identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresDataRightsBacklogEvidence {
    active_request_count: u64,
    pending_propagation_count: u64,
    quarantined_propagation_count: u64,
    oldest_active_request_at_unix_ms: Option<u64>,
    oldest_pending_propagation_event_at_unix_ms: Option<u64>,
}

impl PostgresDataRightsBacklogEvidence {
    /// Return the number of requested, identity-verified, or processing requests.
    #[must_use]
    pub const fn active_request_count(self) -> u64 {
        self.active_request_count
    }

    /// Return the number of downstream propagation records awaiting delivery.
    #[must_use]
    pub const fn pending_propagation_count(self) -> u64 {
        self.pending_propagation_count
    }

    /// Return the number of downstream propagation records quarantined for intervention.
    #[must_use]
    pub const fn quarantined_propagation_count(self) -> u64 {
        self.quarantined_propagation_count
    }

    /// Return the oldest original request time among non-terminal participant requests.
    #[must_use]
    pub const fn oldest_active_request_at_unix_ms(self) -> Option<u64> {
        self.oldest_active_request_at_unix_ms
    }

    /// Return the oldest event time among pending downstream propagations.
    #[must_use]
    pub const fn oldest_pending_propagation_event_at_unix_ms(self) -> Option<u64> {
        self.oldest_pending_propagation_event_at_unix_ms
    }
}

/// Operator-supplied bounds for asynchronous scoring-job backlog evidence.
///
/// Scoring timeliness is a deployment-profile policy. The product does not invent a
/// universal queue-depth or age SLO; callers supply evidence-backed limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringJobBacklogPolicy {
    /// Largest queued, leased, or retry-scheduled job population still accepted.
    pub max_active_job_count: u64,
    /// Largest age in milliseconds of the oldest active scoring job.
    pub max_active_job_age_ms: u64,
    /// Largest quarantined scoring-job population still accepted.
    pub max_quarantined_job_count: u64,
}

/// Aggregate scoring-job backlog evidence without request, worker, or result identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresScoringJobBacklogEvidence {
    active_job_count: u64,
    quarantined_job_count: u64,
    oldest_active_job_at_unix_ms: Option<u64>,
}

impl PostgresScoringJobBacklogEvidence {
    /// Return queued plus leased plus retry-scheduled scoring jobs.
    #[must_use]
    pub const fn active_job_count(self) -> u64 {
        self.active_job_count
    }

    /// Return the number of scoring jobs quarantined for operator attention.
    #[must_use]
    pub const fn quarantined_job_count(self) -> u64 {
        self.quarantined_job_count
    }

    /// Return the oldest `created_at` among active scoring jobs, if any.
    #[must_use]
    pub const fn oldest_active_job_at_unix_ms(self) -> Option<u64> {
        self.oldest_active_job_at_unix_ms
    }
}

/// Fail-closed error while reading durable backlog evidence.
#[derive(Debug)]
#[non_exhaustive]
pub enum PostgresBacklogProbeError {
    /// A stored event time violated the positive-millisecond persistence contract.
    InvalidStoredValue,
    /// `PostgreSQL` could not execute the aggregate backlog probe.
    Database(postgres::Error),
}

impl Display for PostgresBacklogProbeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidStoredValue => "stored backlog evidence violates the persistence contract",
            Self::Database(_) => "PostgreSQL backlog probe failed",
        })
    }
}

impl Error for PostgresBacklogProbeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidStoredValue => None,
        }
    }
}

impl From<postgres::Error> for PostgresBacklogProbeError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Classify server-version and transaction-read-only evidence without performing I/O.
///
/// `PostgreSQL` 10 and later encode `server_version_num` as `major * 10000 + minor`, so
/// integer division yields the server major used by the repository support policy.
#[must_use]
pub const fn classify_postgres_runtime(
    server_version_num: i32,
    transaction_read_only: bool,
) -> PostgresRuntimeHealth {
    let server_major_version = server_version_num / 10_000;
    let status = if server_major_version != SUPPORTED_POSTGRES_MAJOR {
        PostgresRuntimeStatus::UnsupportedMajorVersion
    } else if transaction_read_only {
        PostgresRuntimeStatus::ReadOnly
    } else {
        PostgresRuntimeStatus::Ready
    };
    PostgresRuntimeHealth {
        server_major_version,
        status,
    }
}

/// Probe the caller-owned `PostgreSQL` connection for supported-major and write readiness.
///
/// The probe reads only `PostgreSQL` server settings and never returns credentials,
/// assessment content, tenant identifiers, or restricted linkage data. Callers must map
/// the returned database error to an operator-safe error class before exposing it across
/// a public health endpoint.
///
/// # Errors
///
/// Returns the `PostgreSQL` driver error when the server cannot provide the required
/// settings. Failure to probe must be treated as unknown/unready by the caller.
pub fn probe_postgres_runtime(
    client: &mut impl GenericClient,
) -> Result<PostgresRuntimeHealth, postgres::Error> {
    let row = client.query_one(
        "SELECT current_setting('server_version_num')::integer, \
                current_setting('transaction_read_only')::boolean",
        &[],
    )?;
    let server_version_num: i32 = row.get(0);
    let transaction_read_only: bool = row.get(1);
    Ok(classify_postgres_runtime(
        server_version_num,
        transaction_read_only,
    ))
}

/// Apply partial indexes that keep operational-backlog readiness probes bounded.
///
/// Call this after the integration, inbox-consumption, and data-rights migrations
/// so an existing installation receives the same readiness indexes as a newly
/// initialized database. The statements are idempotent (`CREATE INDEX IF NOT EXISTS`).
/// Directory-order recovery still applies `0020` as a file; this function is the
/// product-owned apply path that callers must use instead of a private `include_str!`.
///
/// # Errors
///
/// Returns the `PostgreSQL` driver error when a required relation is missing or
/// the index statements cannot be executed.
pub fn apply_backlog_health_index_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(BACKLOG_HEALTH_INDEX_MIGRATION)
}

/// Probe aggregate outbox and inbox-consumption backlog evidence in one database snapshot.
///
/// This function intentionally does not classify the result against a universal threshold.
/// It reads only aggregate counts and the oldest server-authoritative event time from the
/// existing product-owned integration tables. Missing tables or query failures are errors,
/// and invalid non-positive stored event times fail closed instead of being normalized.
///
/// # Errors
///
/// Returns [`PostgresBacklogProbeError::InvalidStoredValue`] when a stored oldest-event
/// timestamp violates the positive-millisecond contract, or
/// [`PostgresBacklogProbeError::Database`] when the aggregate query cannot be executed.
pub fn probe_postgres_integration_backlog(
    client: &mut impl GenericClient,
) -> Result<PostgresIntegrationBacklogEvidence, PostgresBacklogProbeError> {
    let row = client.query_one(
        "SELECT \
             (SELECT COUNT(*)::BIGINT FROM integration_outbox WHERE current_state = 'pending'), \
             (SELECT COUNT(*)::BIGINT FROM integration_outbox WHERE current_state = 'quarantined'), \
             (SELECT MIN(latest_event_at_unix_ms) FROM integration_outbox WHERE current_state = 'pending'), \
             (SELECT COUNT(*)::BIGINT FROM integration_consumption \
                 WHERE consumption_state IN ('pending', 'processing')), \
             (SELECT COUNT(*)::BIGINT FROM integration_consumption \
                 WHERE consumption_state = 'quarantined'), \
             (SELECT MIN(latest_event_at_unix_ms) FROM integration_consumption \
                 WHERE consumption_state IN ('pending', 'processing'))",
        &[],
    )?;

    let pending_outbox_count: i64 = row.get(0);
    let quarantined_outbox_count: i64 = row.get(1);
    let oldest_pending_outbox_event_at_unix_ms: Option<i64> = row.get(2);
    let active_consumption_count: i64 = row.get(3);
    let quarantined_consumption_count: i64 = row.get(4);
    let oldest_active_consumption_event_at_unix_ms: Option<i64> = row.get(5);

    Ok(PostgresIntegrationBacklogEvidence {
        pending_outbox_count: pending_outbox_count.cast_unsigned(),
        quarantined_outbox_count: quarantined_outbox_count.cast_unsigned(),
        active_consumption_count: active_consumption_count.cast_unsigned(),
        quarantined_consumption_count: quarantined_consumption_count.cast_unsigned(),
        oldest_pending_outbox_event_at_unix_ms: positive_optional_millis(
            oldest_pending_outbox_event_at_unix_ms,
        )?,
        oldest_active_consumption_event_at_unix_ms: positive_optional_millis(
            oldest_active_consumption_event_at_unix_ms,
        )?,
    })
}

/// Classify aggregate integration backlog evidence against one explicit operator policy.
///
/// A zero observation time or a stored event apparently occurring after the observation
/// is `Unknown`, not healthy. Otherwise any count or age beyond the supplied policy is
/// `Stalled`. No default SLO, alert threshold, or recovery promise is inferred here.
#[must_use]
pub fn classify_postgres_integration_backlog(
    evidence: &PostgresIntegrationBacklogEvidence,
    observed_at_unix_ms: u64,
    policy: &IntegrationBacklogPolicy,
) -> BacklogHealth {
    if observed_at_unix_ms == 0
        || evidence
            .oldest_pending_outbox_event_at_unix_ms
            .is_some_and(|timestamp| timestamp > observed_at_unix_ms)
        || evidence
            .oldest_active_consumption_event_at_unix_ms
            .is_some_and(|timestamp| timestamp > observed_at_unix_ms)
    {
        return BacklogHealth::Unknown;
    }

    if evidence.pending_outbox_count > policy.max_pending_outbox_count
        || evidence.quarantined_outbox_count > policy.max_quarantined_outbox_count
        || evidence.active_consumption_count > policy.max_active_consumption_count
        || evidence.quarantined_consumption_count > policy.max_quarantined_consumption_count
    {
        return BacklogHealth::Stalled;
    }

    if age_exceeds(
        evidence.oldest_pending_outbox_event_at_unix_ms,
        observed_at_unix_ms,
        policy.max_pending_outbox_age_ms,
    ) || age_exceeds(
        evidence.oldest_active_consumption_event_at_unix_ms,
        observed_at_unix_ms,
        policy.max_active_consumption_age_ms,
    ) {
        return BacklogHealth::Stalled;
    }

    BacklogHealth::WithinBounds
}

/// Probe participant data-rights request and propagation backlog evidence.
///
/// Active request age is measured from the original request time, so a recent lifecycle
/// transition cannot hide a participant request that has remained unresolved for too long.
/// Propagation age is measured from the latest durable propagation event. The query exposes
/// only aggregate counts and timestamps, never participant, tenant, scope, or payload data.
///
/// # Errors
///
/// Returns [`PostgresBacklogProbeError::InvalidStoredValue`] for non-positive oldest
/// timestamps, or [`PostgresBacklogProbeError::Database`] when the query cannot execute.
pub fn probe_postgres_data_rights_backlog(
    client: &mut impl GenericClient,
) -> Result<PostgresDataRightsBacklogEvidence, PostgresBacklogProbeError> {
    let row = client.query_one(
        "SELECT \
             (SELECT COUNT(*)::BIGINT FROM data_rights_request_state \
                 WHERE current_state IN ('requested', 'identity_verified', 'processing')), \
             (SELECT MIN(requested_at_unix_ms) FROM data_rights_request_state \
                 WHERE current_state IN ('requested', 'identity_verified', 'processing')), \
             (SELECT COUNT(*)::BIGINT FROM data_rights_propagation_state \
                 WHERE current_state = 'pending'), \
             (SELECT COUNT(*)::BIGINT FROM data_rights_propagation_state \
                 WHERE current_state = 'quarantined'), \
             (SELECT MIN(latest_event_at_unix_ms) FROM data_rights_propagation_state \
                 WHERE current_state = 'pending')",
        &[],
    )?;

    let active_request_count: i64 = row.get(0);
    let oldest_active_request_at_unix_ms: Option<i64> = row.get(1);
    let pending_propagation_count: i64 = row.get(2);
    let quarantined_propagation_count: i64 = row.get(3);
    let oldest_pending_propagation_event_at_unix_ms: Option<i64> = row.get(4);

    Ok(PostgresDataRightsBacklogEvidence {
        active_request_count: active_request_count.cast_unsigned(),
        pending_propagation_count: pending_propagation_count.cast_unsigned(),
        quarantined_propagation_count: quarantined_propagation_count.cast_unsigned(),
        oldest_active_request_at_unix_ms: positive_optional_millis(
            oldest_active_request_at_unix_ms,
        )?,
        oldest_pending_propagation_event_at_unix_ms: positive_optional_millis(
            oldest_pending_propagation_event_at_unix_ms,
        )?,
    })
}

/// Classify participant data-rights backlog evidence against an explicit operator policy.
///
/// Missing observation time or future-dated durable evidence produces `Unknown`. Excess
/// request/propagation counts, quarantine population, or age produce `Stalled`. Otherwise
/// the measured data-rights backlog is within the caller's declared operating bounds.
#[must_use]
pub fn classify_postgres_data_rights_backlog(
    evidence: &PostgresDataRightsBacklogEvidence,
    observed_at_unix_ms: u64,
    policy: &DataRightsBacklogPolicy,
) -> BacklogHealth {
    if observed_at_unix_ms == 0
        || evidence
            .oldest_active_request_at_unix_ms
            .is_some_and(|timestamp| timestamp > observed_at_unix_ms)
        || evidence
            .oldest_pending_propagation_event_at_unix_ms
            .is_some_and(|timestamp| timestamp > observed_at_unix_ms)
    {
        return BacklogHealth::Unknown;
    }

    if evidence.active_request_count > policy.max_active_request_count
        || evidence.pending_propagation_count > policy.max_pending_propagation_count
        || evidence.quarantined_propagation_count > policy.max_quarantined_propagation_count
    {
        return BacklogHealth::Stalled;
    }

    if age_exceeds(
        evidence.oldest_active_request_at_unix_ms,
        observed_at_unix_ms,
        policy.max_active_request_age_ms,
    ) || age_exceeds(
        evidence.oldest_pending_propagation_event_at_unix_ms,
        observed_at_unix_ms,
        policy.max_pending_propagation_age_ms,
    ) {
        return BacklogHealth::Stalled;
    }

    BacklogHealth::WithinBounds
}

/// Probe aggregate scoring-job backlog evidence without identities or payloads.
///
/// Active work is queued, leased, or retry-scheduled. Completed and cancelled jobs are
/// terminal and do not participate. Age is measured from `created_at` so a later lease
/// or retry transition cannot hide a job that has waited too long. The query returns
/// only counts and one oldest timestamp.
///
/// # Errors
///
/// Returns [`PostgresBacklogProbeError::InvalidStoredValue`] when the oldest created
/// time is not a positive unix-millisecond value, or
/// [`PostgresBacklogProbeError::Database`] when the query cannot execute.
pub fn probe_postgres_scoring_job_backlog(
    client: &mut impl GenericClient,
) -> Result<PostgresScoringJobBacklogEvidence, PostgresBacklogProbeError> {
    let row = client.query_one(
        "SELECT \
             (SELECT COUNT(*)::BIGINT FROM scoring_job_state \
                 WHERE scoring_state IN ('queued', 'leased', 'retry_scheduled')), \
             (SELECT COUNT(*)::BIGINT FROM scoring_job_state \
                 WHERE scoring_state = 'quarantined'), \
             (SELECT (EXTRACT(EPOCH FROM MIN(created_at)) * 1000)::BIGINT \
                 FROM scoring_job_state \
                 WHERE scoring_state IN ('queued', 'leased', 'retry_scheduled'))",
        &[],
    )?;

    let active_job_count: i64 = row.get(0);
    let quarantined_job_count: i64 = row.get(1);
    let oldest_active_job_at_unix_ms: Option<i64> = row.get(2);

    Ok(PostgresScoringJobBacklogEvidence {
        active_job_count: active_job_count.cast_unsigned(),
        quarantined_job_count: quarantined_job_count.cast_unsigned(),
        oldest_active_job_at_unix_ms: positive_optional_millis(oldest_active_job_at_unix_ms)?,
    })
}

/// Classify scoring-job backlog evidence against an explicit operator policy.
///
/// Missing observation time or future-dated created-at evidence produces `Unknown`.
/// Excess active or quarantined counts, or an active job older than the supplied
/// age bound, produce `Stalled`.
#[must_use]
pub fn classify_postgres_scoring_job_backlog(
    evidence: &PostgresScoringJobBacklogEvidence,
    observed_at_unix_ms: u64,
    policy: &ScoringJobBacklogPolicy,
) -> BacklogHealth {
    if observed_at_unix_ms == 0
        || evidence
            .oldest_active_job_at_unix_ms
            .is_some_and(|timestamp| timestamp > observed_at_unix_ms)
    {
        return BacklogHealth::Unknown;
    }

    if evidence.active_job_count > policy.max_active_job_count
        || evidence.quarantined_job_count > policy.max_quarantined_job_count
    {
        return BacklogHealth::Stalled;
    }

    if age_exceeds(
        evidence.oldest_active_job_at_unix_ms,
        observed_at_unix_ms,
        policy.max_active_job_age_ms,
    ) {
        return BacklogHealth::Stalled;
    }

    BacklogHealth::WithinBounds
}

fn positive_optional_millis(value: Option<i64>) -> Result<Option<u64>, PostgresBacklogProbeError> {
    match value {
        Some(value) if value > 0 => Ok(Some(value.cast_unsigned())),
        Some(_) => Err(PostgresBacklogProbeError::InvalidStoredValue),
        None => Ok(None),
    }
}

fn age_exceeds(oldest_event_at: Option<u64>, observed_at: u64, maximum_age: u64) -> bool {
    oldest_event_at.is_some_and(|timestamp| observed_at.saturating_sub(timestamp) > maximum_age)
}

/// Probe whether every caller-declared relation required by this application build exists.
///
/// The application or packaged deployment remains responsible for declaring the exact
/// relation set that represents its compatible schema version. Relation names are passed
/// as query parameters, never interpolated into SQL. A missing required relation is a
/// known incompatibility and therefore fails state-changing readiness closed through
/// [`DataIntegrityHealth::Incompatible`]. An empty requirement set is vacuously verified.
///
/// This probe deliberately does not claim that relation presence alone proves migration,
/// column, constraint, digest, tenant, or provenance integrity. Those stronger invariants
/// remain separate evidence and must not be promoted from this narrow check.
///
/// # Errors
///
/// Returns the `PostgreSQL` driver error when relation existence cannot be established.
/// Callers must map probe failure to unknown/unready data-integrity state and avoid
/// exposing raw database errors on public health endpoints.
pub fn probe_postgres_relation_integrity(
    client: &mut impl GenericClient,
    required_relations: &[&str],
) -> Result<DataIntegrityHealth, postgres::Error> {
    for relation in required_relations {
        let row = client.query_one("SELECT to_regclass($1) IS NOT NULL", &[relation])?;
        let exists: bool = row.get(0);
        if !exists {
            return Ok(DataIntegrityHealth::Incompatible);
        }
    }
    Ok(DataIntegrityHealth::Verified)
}
