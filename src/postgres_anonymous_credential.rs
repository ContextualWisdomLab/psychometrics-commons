//! PostgreSQL persistence for short-lived anonymous assessment credentials.
//!
//! The adapter stores only the already-hashed proof from [`AnonymousCredential`]. Raw bearer
//! secrets remain outside the product database and routine logs. Issue evidence is immutable;
//! revocation is the only allowed semantic update and may be recorded once. Exact issue and
//! revocation replays are idempotent, while rebinding a credential or proof digest fails closed.
//!
//! Callers own the database connection, credentials, and transaction boundary. Persistence uses
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race becomes visible to the
//! replay classifier on the next statement.

use crate::anonymous_credential::AnonymousCredential;
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const ANONYMOUS_CREDENTIAL_MIGRATION: &str =
    include_str!("../migrations/0040_anonymous_session_credential.sql");

/// Outcome of persisting one immutable anonymous credential issue record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousCredentialPersistenceDisposition {
    /// A new credential issue record was inserted.
    Inserted,
    /// The exact immutable issue evidence already existed.
    Duplicate,
}

/// Outcome of persisting append-only anonymous credential revocation evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousCredentialRevocationDisposition {
    /// Revocation evidence was recorded for the first time.
    Revoked,
    /// The exact same revocation evidence was already durable.
    Duplicate,
}

/// Fail-closed error for durable anonymous credential persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnonymousCredentialPersistenceError {
    /// A supplied public reference was not an exact opaque non-numeric reference.
    InvalidReference,
    /// Issuance was attempted with a revoked credential, or revocation without revocation evidence.
    InvalidCredentialState,
    /// Revocation targeted a credential that does not exist in the supplied tenant.
    MissingCredential,
    /// A credential or proof identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A domain timestamp does not fit the PostgreSQL `BIGINT` storage contract.
    ValueOutOfRange,
    /// Stored rows cannot reconstruct a valid anonymous credential.
    InconsistentEvidence,
    /// Credential persistence requires PostgreSQL `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// PostgreSQL rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for AnonymousCredentialPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "anonymous credential persistence references must be exact opaque values"
            }
            Self::InvalidCredentialState => {
                "anonymous credential persistence received the wrong lifecycle state"
            }
            Self::MissingCredential => {
                "anonymous credential revocation requires an existing tenant-bound issue record"
            }
            Self::ConflictingReplay => {
                "anonymous credential identity was replayed with conflicting evidence"
            }
            Self::ValueOutOfRange => {
                "anonymous credential timestamp exceeds the PostgreSQL bigint range"
            }
            Self::InconsistentEvidence => {
                "durable anonymous credential evidence cannot reconstruct a valid credential"
            }
            Self::UnsupportedIsolationLevel => {
                "anonymous credential persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL anonymous credential persistence failed",
        })
    }
}

impl Error for AnonymousCredentialPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for AnonymousCredentialPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent anonymous-credential migration to a PostgreSQL connection.
///
/// # Errors
///
/// Returns the PostgreSQL error if the schema, constraints, indexes, or immutability triggers
/// cannot be created.
pub fn apply_anonymous_credential_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ANONYMOUS_CREDENTIAL_MIGRATION)
}

/// Persist one newly issued short-lived anonymous credential.
///
/// The raw bearer proof is never accepted here. [`AnonymousCredential`] carries only its canonical
/// SHA-256 digest plus the exact tenant, participant, session, and lifetime binding. The same issue
/// evidence may be replayed idempotently. Reusing either the credential reference or proof digest
/// with different evidence fails closed.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError`] when the transaction isolation is unsupported,
/// the supplied credential is already revoked, a timestamp cannot fit PostgreSQL, a replay
/// conflicts with durable evidence, or PostgreSQL fails.
pub fn persist_anonymous_credential_issue(
    transaction: &mut Transaction<'_>,
    credential: &AnonymousCredential,
) -> Result<AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError> {
    require_read_committed(transaction)?;
    if credential.revoked_at_unix_ms().is_some() {
        return Err(AnonymousCredentialPersistenceError::InvalidCredentialState);
    }
    let issued_at = stored_timestamp(credential.issued_at_unix_ms())?;
    let expires_at = stored_timestamp(credential.expires_at_unix_ms())?;

    let inserted = transaction.execute(
        "INSERT INTO anonymous_session_credential (\
             credential_ref, tenant_ref, participant_ref, session_ref, proof_digest, \
             issued_at_unix_ms, expires_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT DO NOTHING",
        &[
            &credential.credential_ref(),
            &credential.tenant_ref(),
            &credential.participant_ref(),
            &credential.session_ref(),
            &credential.proof_digest(),
            &issued_at,
            &expires_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(AnonymousCredentialPersistenceDisposition::Inserted);
    }

    classify_issue_replay(transaction, credential, issued_at, expires_at)
}

/// Persist the first append-only revocation timestamp for one issued anonymous credential.
///
/// The credential must already exist with the exact original issue evidence. Replaying the exact
/// revocation timestamp is idempotent. A different revocation time or any issue-evidence rebinding
/// fails closed instead of rewriting history.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError`] when the transaction isolation is unsupported,
/// the supplied value has no revocation evidence, the issue record is missing, a timestamp cannot
/// fit PostgreSQL, the replay conflicts, or PostgreSQL fails.
pub fn persist_anonymous_credential_revocation(
    transaction: &mut Transaction<'_>,
    credential: &AnonymousCredential,
) -> Result<AnonymousCredentialRevocationDisposition, AnonymousCredentialPersistenceError> {
    require_read_committed(transaction)?;
    let revoked_at = credential
        .revoked_at_unix_ms()
        .ok_or(AnonymousCredentialPersistenceError::InvalidCredentialState)
        .and_then(stored_timestamp)?;
    let issued_at = stored_timestamp(credential.issued_at_unix_ms())?;
    let expires_at = stored_timestamp(credential.expires_at_unix_ms())?;

    let row = transaction.query_opt(
        "SELECT participant_ref, session_ref, proof_digest, issued_at_unix_ms, \
                expires_at_unix_ms, revoked_at_unix_ms \
         FROM anonymous_session_credential \
         WHERE credential_ref = $1 AND tenant_ref = $2 \
         FOR UPDATE",
        &[&credential.credential_ref(), &credential.tenant_ref()],
    )?;
    let Some(row) = row else {
        return Err(AnonymousCredentialPersistenceError::MissingCredential);
    };

    let stored_participant: String = row.get(0);
    let stored_session: String = row.get(1);
    let stored_digest: String = row.get(2);
    let stored_issued: i64 = row.get(3);
    let stored_expires: i64 = row.get(4);
    let stored_revoked: Option<i64> = row.get(5);
    if stored_participant != credential.participant_ref()
        || stored_session != credential.session_ref()
        || stored_digest != credential.proof_digest()
        || stored_issued != issued_at
        || stored_expires != expires_at
    {
        return Err(AnonymousCredentialPersistenceError::ConflictingReplay);
    }

    match stored_revoked {
        Some(existing) if existing == revoked_at => {
            Ok(AnonymousCredentialRevocationDisposition::Duplicate)
        }
        Some(_) => Err(AnonymousCredentialPersistenceError::ConflictingReplay),
        None => {
            transaction.execute(
                "UPDATE anonymous_session_credential \
                 SET revoked_at_unix_ms = $1 \
                 WHERE credential_ref = $2 AND tenant_ref = $3",
                &[&revoked_at, &credential.credential_ref(), &credential.tenant_ref()],
            )?;
            Ok(AnonymousCredentialRevocationDisposition::Revoked)
        }
    }
}

/// Reload one tenant-bound anonymous credential from durable evidence.
///
/// A missing credential or a credential belonging to another tenant returns `Ok(None)`. Stored
/// fields are reconstructed through the domain constructor and revocation transition so corrupted
/// rows cannot silently become usable authority after restart.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError`] for unsupported isolation, malformed lookup
/// references, corrupt durable evidence, or a PostgreSQL failure.
pub fn load_anonymous_credential(
    transaction: &mut Transaction<'_>,
    credential_ref: &str,
    tenant_ref: &str,
) -> Result<Option<AnonymousCredential>, AnonymousCredentialPersistenceError> {
    require_read_committed(transaction)?;
    let credential_ref = required_reference(credential_ref)?;
    let tenant_ref = required_reference(tenant_ref)?;
    let row = transaction.query_opt(
        "SELECT participant_ref, session_ref, proof_digest, issued_at_unix_ms, \
                expires_at_unix_ms, revoked_at_unix_ms \
         FROM anonymous_session_credential \
         WHERE credential_ref = $1 AND tenant_ref = $2",
        &[&credential_ref, &tenant_ref],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };

    let participant_ref: String = row.get(0);
    let session_ref: String = row.get(1);
    let proof_digest: String = row.get(2);
    let issued_at = domain_timestamp(row.get(3))?;
    let expires_at = domain_timestamp(row.get(4))?;
    let revoked_at: Option<i64> = row.get(5);
    let mut credential = AnonymousCredential::new(
        credential_ref,
        tenant_ref,
        &participant_ref,
        &session_ref,
        &proof_digest,
        issued_at,
        expires_at,
    )
    .map_err(|_| AnonymousCredentialPersistenceError::InconsistentEvidence)?;
    if let Some(revoked_at) = revoked_at {
        credential
            .revoke(domain_timestamp(revoked_at)?)
            .map_err(|_| AnonymousCredentialPersistenceError::InconsistentEvidence)?;
    }
    Ok(Some(credential))
}

fn classify_issue_replay(
    transaction: &mut Transaction<'_>,
    credential: &AnonymousCredential,
    issued_at: i64,
    expires_at: i64,
) -> Result<AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError> {
    let row = transaction.query_opt(
        "SELECT tenant_ref, participant_ref, session_ref, proof_digest, issued_at_unix_ms, \
                expires_at_unix_ms, revoked_at_unix_ms \
         FROM anonymous_session_credential \
         WHERE credential_ref = $1",
        &[&credential.credential_ref()],
    )?;
    if let Some(row) = row {
        let stored_tenant: String = row.get(0);
        let stored_participant: String = row.get(1);
        let stored_session: String = row.get(2);
        let stored_digest: String = row.get(3);
        let stored_issued: i64 = row.get(4);
        let stored_expires: i64 = row.get(5);
        let stored_revoked: Option<i64> = row.get(6);
        if stored_tenant == credential.tenant_ref()
            && stored_participant == credential.participant_ref()
            && stored_session == credential.session_ref()
            && stored_digest == credential.proof_digest()
            && stored_issued == issued_at
            && stored_expires == expires_at
            && stored_revoked.is_none()
        {
            return Ok(AnonymousCredentialPersistenceDisposition::Duplicate);
        }
        return Err(AnonymousCredentialPersistenceError::ConflictingReplay);
    }

    let proof_reused = transaction.query_opt(
        "SELECT credential_ref FROM anonymous_session_credential WHERE proof_digest = $1",
        &[&credential.proof_digest()],
    )?;
    if proof_reused.is_some() {
        Err(AnonymousCredentialPersistenceError::ConflictingReplay)
    } else {
        Err(AnonymousCredentialPersistenceError::InconsistentEvidence)
    }
}

fn required_reference(reference: &str) -> Result<&str, AnonymousCredentialPersistenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AnonymousCredentialPersistenceError::InvalidReference),
    }
}

fn stored_timestamp(timestamp: u64) -> Result<i64, AnonymousCredentialPersistenceError> {
    i64::try_from(timestamp).map_err(|_| AnonymousCredentialPersistenceError::ValueOutOfRange)
}

fn domain_timestamp(timestamp: i64) -> Result<u64, AnonymousCredentialPersistenceError> {
    u64::try_from(timestamp).map_err(|_| AnonymousCredentialPersistenceError::InconsistentEvidence)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), AnonymousCredentialPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(AnonymousCredentialPersistenceError::UnsupportedIsolationLevel)
    }
}
