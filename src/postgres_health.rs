//! `PostgreSQL` operational-store compatibility and write-readiness evidence.
//!
//! The runtime's initial persistence contract supports upstream `PostgreSQL` 18.x
//! databases using UTF8 server encoding. This module probes the caller-owned connection
//! and classifies whether the database can safely accept product-owned state changes. It
//! does not own credentials, connection pooling, migrations, backup, or recovery.

use crate::health::{CapabilityHealth, CapabilityState, DataIntegrityHealth, HealthContractError};
use postgres::GenericClient;

/// Initial supported `PostgreSQL` server major version from ADR-0015.
pub const SUPPORTED_POSTGRES_MAJOR: i32 = 18;

/// Required `PostgreSQL` server encoding for Unicode-aware database constraints.
pub const SUPPORTED_POSTGRES_ENCODING: &str = "UTF8";

/// Capability identity used by operation-scoped runtime readiness.
pub const POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF: &str = "postgres_operational_store";

/// Fail-closed classification of the product operational database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PostgresRuntimeStatus {
    /// The supported `PostgreSQL` major and UTF8 encoding are reachable and accept writes.
    Ready,
    /// The `PostgreSQL` server major is outside the repository's validated support boundary.
    UnsupportedMajorVersion,
    /// The server encoding cannot safely execute the repository's Unicode-aware constraints.
    UnsupportedEncoding,
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
            PostgresRuntimeStatus::UnsupportedMajorVersion
            | PostgresRuntimeStatus::UnsupportedEncoding
            | PostgresRuntimeStatus::ReadOnly => CapabilityState::Unavailable,
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

/// Classify server-version, server-encoding, and transaction-read-only evidence without I/O.
///
/// `PostgreSQL` 10 and later encode `server_version_num` as `major * 10000 + minor`, so
/// integer division yields the server major used by the repository support policy. UTF8 is
/// required because the integration persistence schema uses PostgreSQL's Unicode-aware
/// `pg_unicode_fast` collation for reference validation.
#[must_use]
pub fn classify_postgres_runtime(
    server_version_num: i32,
    server_encoding: &str,
    transaction_read_only: bool,
) -> PostgresRuntimeHealth {
    let server_major_version = server_version_num / 10_000;
    let status = if server_major_version != SUPPORTED_POSTGRES_MAJOR {
        PostgresRuntimeStatus::UnsupportedMajorVersion
    } else if server_encoding != SUPPORTED_POSTGRES_ENCODING {
        PostgresRuntimeStatus::UnsupportedEncoding
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

/// Probe the caller-owned `PostgreSQL` connection for supported-major, UTF8, and write readiness.
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
                current_setting('server_encoding'), \
                current_setting('transaction_read_only')::boolean",
        &[],
    )?;
    let server_version_num: i32 = row.get(0);
    let server_encoding: String = row.get(1);
    let transaction_read_only: bool = row.get(2);
    Ok(classify_postgres_runtime(
        server_version_num,
        &server_encoding,
        transaction_read_only,
    ))
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
