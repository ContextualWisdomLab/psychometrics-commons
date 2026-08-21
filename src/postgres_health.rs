//! `PostgreSQL` operational-store compatibility and write-readiness evidence.
//!
//! The runtime's initial persistence contract supports upstream `PostgreSQL` 18.x.
//! This module probes the caller-owned connection and classifies whether the database
//! can safely accept product-owned state changes. It does not own credentials,
//! connection pooling, migrations, backup, or recovery.

use crate::health::{CapabilityHealth, CapabilityState, DataIntegrityHealth, HealthContractError};
use postgres::GenericClient;

/// Initial supported `PostgreSQL` server major version from ADR-0015.
pub const SUPPORTED_POSTGRES_MAJOR: i32 = 18;

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

/// Probe whether every caller-declared relation required by this application build exists.
///
/// The application or packaged deployment remains responsible for declaring the exact
/// relation set that represents its compatible schema version. Each relation identity must
/// be an exact lowercase two-part `schema.relation` name with no whitespace. PostgreSQL's
/// `search_path` is the ordered list of schemas used to resolve an unqualified name, so
/// requiring the schema explicitly prevents the answer from changing with that setting.
/// Lowercase spelling prevents PostgreSQL from silently folding an alias such as
/// `Public.MyTable` to a different lowercase identity. Relation names are passed as query
/// parameters, never interpolated into SQL.
///
/// Here, *integrity evidence* means the observed fact that every required relation exists.
/// *State-changing readiness* means whether the product may accept new writes. The probe
/// *fails closed*: malformed or missing evidence returns [`DataIntegrityHealth::Incompatible`]
/// (or [`DataIntegrityHealth::Unknown`] when no requirement set was supplied), so callers
/// deny write readiness instead of assuming the schema is safe.
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
    if required_relations.is_empty() {
        return Ok(DataIntegrityHealth::Unknown);
    }

    for relation in required_relations {
        if !is_exact_schema_qualified_relation(relation) {
            return Ok(DataIntegrityHealth::Incompatible);
        }
        let row = client.query_one("SELECT to_regclass($1) IS NOT NULL", &[relation])?;
        let exists: bool = row.get(0);
        if !exists {
            return Ok(DataIntegrityHealth::Incompatible);
        }
    }
    Ok(DataIntegrityHealth::Verified)
}

fn is_exact_schema_qualified_relation(relation: &str) -> bool {
    if relation.chars().any(char::is_whitespace) || relation != relation.to_ascii_lowercase() {
        return false;
    }
    let Some((schema, name)) = relation.split_once('.') else {
        return false;
    };
    !schema.is_empty() && !name.is_empty() && !name.contains('.')
}
