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
/// required because the integration persistence schema uses `PostgreSQL`'s Unicode-aware
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
/// relation set that represents its compatible schema version. Each relation identity must
/// be an exact two-part `schema.relation` name using the repository's unquoted ASCII SQL
/// identifier grammar: lowercase letters, digits, and underscores, with each component
/// starting with a lowercase letter or underscore and no component exceeding `PostgreSQL`'s
/// 63-byte identifier limit. `PostgreSQL`'s `search_path` is the ordered list of schemas
/// used to resolve an unqualified name, so requiring the schema explicitly prevents the
/// answer from changing with that setting. Restricting the input to this ASCII grammar also
/// prevents case-folding, truncation, or Unicode-confusable aliases from resolving to an
/// identity different from the declared one. Relation names are passed as query parameters,
/// never interpolated into SQL.
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
    let Some((schema, name)) = relation.split_once('.') else {
        return false;
    };
    is_exact_unquoted_identifier(schema) && is_exact_unquoted_identifier(name)
}

fn is_exact_unquoted_identifier(identifier: &str) -> bool {
    if identifier.len() > 63 {
        return false;
    }
    let mut bytes = identifier.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use postgres::{Client, NoTls};

    fn test_client() -> Client {
        let connection = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
        Client::connect(&connection, NoTls)
            .expect("isolated CI PostgreSQL database must be reachable")
    }

    #[test]
    fn runtime_health_covers_supported_read_only_and_unsupported_states() {
        let ready = classify_postgres_runtime(180_004, "UTF8", false);
        assert_eq!(ready.server_major_version(), SUPPORTED_POSTGRES_MAJOR);
        assert_eq!(ready.status(), PostgresRuntimeStatus::Ready);
        assert_eq!(ready.capability_state(), CapabilityState::Available);
        assert!(ready.accepts_new_work());
        assert!(ready.capability_health().unwrap().accepts_new_work());

        let read_only = classify_postgres_runtime(180_004, "UTF8", true);
        assert_eq!(read_only.status(), PostgresRuntimeStatus::ReadOnly);
        assert_eq!(read_only.capability_state(), CapabilityState::Unavailable);
        assert!(!read_only.accepts_new_work());
        assert!(!read_only.capability_health().unwrap().accepts_new_work());

        let unsupported = classify_postgres_runtime(170_009, "UTF8", false);
        assert_eq!(
            unsupported.status(),
            PostgresRuntimeStatus::UnsupportedMajorVersion
        );
        assert_eq!(unsupported.capability_state(), CapabilityState::Unavailable);
        assert!(!unsupported.accepts_new_work());
    }

    #[test]
    fn live_runtime_probe_covers_success_read_only_and_database_error() {
        let mut client = test_client();
        let ready = probe_postgres_runtime(&mut client).unwrap();
        assert_eq!(ready.status(), PostgresRuntimeStatus::Ready);

        let mut transaction = client.build_transaction().read_only(true).start().unwrap();
        let read_only = probe_postgres_runtime(&mut transaction).unwrap();
        assert_eq!(read_only.status(), PostgresRuntimeStatus::ReadOnly);
        transaction.rollback().unwrap();

        let mut transaction = client.transaction().unwrap();
        assert!(transaction.batch_execute("SELECT 1 / 0").is_err());
        assert!(probe_postgres_runtime(&mut transaction).is_err());
    }

    #[test]
    fn relation_probe_covers_unknown_verified_incompatible_and_database_error() {
        let mut client = test_client();
        assert_eq!(
            probe_postgres_relation_integrity(&mut client, &[]).unwrap(),
            DataIntegrityHealth::Unknown
        );
        assert_eq!(
            probe_postgres_relation_integrity(&mut client, &["pg_catalog.pg_class"]).unwrap(),
            DataIntegrityHealth::Verified
        );
        assert_eq!(
            probe_postgres_relation_integrity(&mut client, &["pg_class"]).unwrap(),
            DataIntegrityHealth::Incompatible
        );
        assert_eq!(
            probe_postgres_relation_integrity(
                &mut client,
                &["public.psychometrics_commons_missing_relation"]
            )
            .unwrap(),
            DataIntegrityHealth::Incompatible
        );

        let mut transaction = client.transaction().unwrap();
        assert!(transaction.batch_execute("SELECT 1 / 0").is_err());
        assert!(
            probe_postgres_relation_integrity(&mut transaction, &["pg_catalog.pg_class"]).is_err()
        );
    }

    #[test]
    fn exact_relation_identifier_contract_covers_component_boundaries() {
        assert!(is_exact_schema_qualified_relation("pg_catalog.pg_class"));
        assert!(is_exact_schema_qualified_relation("_private.table_2"));
        assert!(is_exact_schema_qualified_relation(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.table"
        ));

        for relation in [
            "pg_class",
            ".pg_class",
            "pg_catalog.",
            "pg_catalog.pg_class.extra",
            "PG_catalog.pg_class",
            "pg_catalog.PG_class",
            "pg_catalog. pg_class",
            "pg_catalog.pg-class",
            "pg_catalog.pg$class",
            "pg_catalog.tablé",
            "1schema.table",
            "schema.1table",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.table",
            "schema.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(!is_exact_schema_qualified_relation(relation), "{relation}");
        }
    }
}
