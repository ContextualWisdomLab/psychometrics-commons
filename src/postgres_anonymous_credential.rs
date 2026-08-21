//! `PostgreSQL` 18 persistence for short-lived anonymous credential evidence.
//!
//! The adapter stores only canonical SHA-256 proof digests and exact tenant,
//! participant, and session bindings. Raw bearer proofs stay outside this
//! boundary. Callers own the connection, credentials, and transaction. Replay
//! classification requires `READ COMMITTED` so a concurrent insert or revocation
//! that wins a unique/update race is visible to the exact-replay classifier.

use crate::anonymous_credential::AnonymousCredential;
use crate::reference::normalized_reference;
use postgres::error::SqlState;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const ANONYMOUS_CREDENTIAL_MIGRATION: &str =
    include_str!("../migrations/0020_anonymous_credential_evidence.sql");
const EXISTING_CREDENTIAL_SQL: &str =
    "SELECT tenant_ref, participant_ref, session_ref, proof_digest, issued_at_unix_ms, \
            expires_at_unix_ms, revoked_at_unix_ms \
     FROM anonymous_credential_evidence WHERE credential_ref = $1";
const REVOCATION_UPDATE_SQL: &str =
    "UPDATE anonymous_credential_evidence SET revoked_at_unix_ms = $1 \
     WHERE credential_ref = $2 AND revoked_at_unix_ms IS NULL";
const REVOCATION_STATE_SQL: &str = "SELECT revoked_at_unix_ms FROM anonymous_credential_evidence \
     WHERE credential_ref = $1";

/// Outcome of persisting one anonymous credential record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnonymousCredentialPersistenceDisposition {
    /// A new credential evidence row was inserted.
    Inserted,
    /// The same credential identity, digest, lifetime, and revocation already existed.
    Duplicate,
    /// Append-only revocation evidence was recorded on an existing credential.
    Revoked,
}

/// Fail-closed error for durable anonymous-credential persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnonymousCredentialPersistenceError {
    /// A credential, tenant, participant, or session reference was not canonical.
    InvalidReference,
    /// A presented or stored proof digest was not canonical lowercase SHA-256 evidence.
    InvalidDigest,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// A stored row could not be reconstructed into the domain contract.
    InvalidStoredEvidence,
    /// Credential identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A later revocation tried to replace already-recorded revocation evidence.
    ConflictingRevocation,
    /// Anonymous-credential persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for AnonymousCredentialPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "anonymous credential persistence references must be exact canonical opaque values"
            }
            Self::InvalidDigest => {
                "anonymous credential persistence digest must be canonical lowercase SHA-256 evidence"
            }
            Self::InvalidTimestamp => {
                "anonymous credential persistence timestamp exceeds the PostgreSQL bigint range"
            }
            Self::InvalidStoredEvidence => {
                "anonymous credential stored evidence violated the domain contract"
            }
            Self::ConflictingReplay => {
                "anonymous credential identity was replayed with conflicting evidence"
            }
            Self::ConflictingRevocation => {
                "anonymous credential revocation evidence cannot be replaced"
            }
            Self::UnsupportedIsolationLevel => {
                "anonymous credential persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL anonymous-credential persistence failed",
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

/// Apply the idempotent anonymous-credential migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_anonymous_credential_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(ANONYMOUS_CREDENTIAL_MIGRATION)
}

/// Persist one anonymous credential record and its append-only revocation evidence.
///
/// Exact replay of the same identity, digest, lifetime, and revocation is
/// idempotent, including two concurrent attempts to record the same revocation.
/// Rebinding `credential_ref` or `proof_digest` fails closed. Revocation may move
/// from absent to one recorded timestamp; a different timestamp cannot replace it.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError`] for unsupported isolation,
/// conflicting replay or revocation, an invalid reference or timestamp, or a
/// database failure.
pub fn persist_anonymous_credential(
    transaction: &mut Transaction<'_>,
    credential: &AnonymousCredential,
) -> Result<AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError> {
    require_read_committed(transaction)?;
    let credential_ref = required_reference(credential.credential_ref())?;
    let tenant_ref = required_reference(credential.tenant_ref())?;
    let participant_ref = required_reference(credential.participant_ref())?;
    let session_ref = required_reference(credential.session_ref())?;
    let proof_digest = required_digest(credential.proof_digest())?;
    let issued_at_unix_ms = postgres_timestamp(credential.issued_at_unix_ms())?;
    let expires_at_unix_ms = postgres_timestamp(credential.expires_at_unix_ms())?;
    let revoked_at_unix_ms = credential
        .revoked_at_unix_ms()
        .map(postgres_timestamp)
        .transpose()?;
    let inserted = match transaction.execute(
        "INSERT INTO anonymous_credential_evidence (\
             credential_ref, tenant_ref, participant_ref, session_ref, proof_digest, \
             issued_at_unix_ms, expires_at_unix_ms, revoked_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (credential_ref) DO NOTHING",
        &[
            &credential_ref,
            &tenant_ref,
            &participant_ref,
            &session_ref,
            &proof_digest,
            &issued_at_unix_ms,
            &expires_at_unix_ms,
            &revoked_at_unix_ms,
        ],
    ) {
        Ok(count) => count,
        Err(error) if is_unique_violation(&error) => {
            return Err(AnonymousCredentialPersistenceError::ConflictingReplay);
        }
        Err(error) => return Err(error.into()),
    };
    if inserted == 1 {
        return Ok(AnonymousCredentialPersistenceDisposition::Inserted);
    }
    classify_existing_credential(transaction, credential, revoked_at_unix_ms)
}

/// Load one credential by its opaque server-side record reference.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError::InvalidReference`] when the
/// lookup reference is not canonical, [`AnonymousCredentialPersistenceError::InvalidStoredEvidence`]
/// when a stored row cannot be reconstructed, or a database failure.
pub fn load_anonymous_credential(
    client: &mut impl GenericClient,
    credential_ref: &str,
) -> Result<Option<AnonymousCredential>, AnonymousCredentialPersistenceError> {
    let credential_ref = required_reference(credential_ref)?;
    let row = client.query_opt(
        "SELECT credential_ref, tenant_ref, participant_ref, session_ref, proof_digest, \
                issued_at_unix_ms, expires_at_unix_ms, revoked_at_unix_ms \
         FROM anonymous_credential_evidence WHERE credential_ref = $1",
        &[&credential_ref],
    )?;
    row.map(|row| credential_from_row(&row)).transpose()
}

/// Load the credential bound to one exact tenant, participant, session, and digest.
///
/// # Errors
///
/// Returns [`AnonymousCredentialPersistenceError::InvalidReference`] or
/// [`AnonymousCredentialPersistenceError::InvalidDigest`] when the presented
/// lookup is not already canonical, stored-evidence reconstruction failures, or
/// a database failure.
pub fn load_anonymous_credential_for_binding(
    client: &mut impl GenericClient,
    tenant_ref: &str,
    participant_ref: &str,
    session_ref: &str,
    proof_digest: &str,
) -> Result<Option<AnonymousCredential>, AnonymousCredentialPersistenceError> {
    let tenant_ref = required_reference(tenant_ref)?;
    let participant_ref = required_reference(participant_ref)?;
    let session_ref = required_reference(session_ref)?;
    let proof_digest = required_digest(proof_digest)?;
    let row = client.query_opt(
        "SELECT credential_ref, tenant_ref, participant_ref, session_ref, proof_digest, \
                issued_at_unix_ms, expires_at_unix_ms, revoked_at_unix_ms \
         FROM anonymous_credential_evidence \
         WHERE tenant_ref = $1 AND participant_ref = $2 AND session_ref = $3 AND proof_digest = $4",
        &[&tenant_ref, &participant_ref, &session_ref, &proof_digest],
    )?;
    row.map(|row| credential_from_row(&row)).transpose()
}

fn classify_existing_credential(
    transaction: &mut Transaction<'_>,
    credential: &AnonymousCredential,
    incoming_revoked_at: Option<i64>,
) -> Result<AnonymousCredentialPersistenceDisposition, AnonymousCredentialPersistenceError> {
    let credential_ref = credential.credential_ref();
    let row = transaction.query_one(EXISTING_CREDENTIAL_SQL, &[&credential_ref])?;
    let stored_tenant: String = row.get(0);
    let stored_participant: String = row.get(1);
    let stored_session: String = row.get(2);
    let stored_digest: String = row.get(3);
    let stored_issued: i64 = row.get(4);
    let stored_expires: i64 = row.get(5);
    let stored_revoked: Option<i64> = row.get(6);
    let issued_at_unix_ms = postgres_timestamp(credential.issued_at_unix_ms())?;
    let expires_at_unix_ms = postgres_timestamp(credential.expires_at_unix_ms())?;
    let identity_matches = stored_tenant == credential.tenant_ref()
        && stored_participant == credential.participant_ref()
        && stored_session == credential.session_ref()
        && stored_digest == credential.proof_digest()
        && stored_issued == issued_at_unix_ms
        && stored_expires == expires_at_unix_ms;
    if !identity_matches {
        return Err(AnonymousCredentialPersistenceError::ConflictingReplay);
    }
    match (stored_revoked, incoming_revoked_at) {
        (None, None) => Ok(AnonymousCredentialPersistenceDisposition::Duplicate),
        (Some(stored), Some(incoming)) if stored == incoming => {
            Ok(AnonymousCredentialPersistenceDisposition::Duplicate)
        }
        (None, Some(revoked_at_unix_ms)) => {
            let updated = transaction.execute(
                REVOCATION_UPDATE_SQL,
                &[&revoked_at_unix_ms, &credential_ref],
            )?;
            if updated == 1 {
                return Ok(AnonymousCredentialPersistenceDisposition::Revoked);
            }

            // Under READ COMMITTED this statement gets a fresh snapshot after a
            // competing updater released the row lock. Reclassify only an exact
            // committed revocation as an idempotent duplicate; a different durable
            // timestamp still fails closed.
            let row = transaction.query_one(REVOCATION_STATE_SQL, &[&credential_ref])?;
            let committed_revoked_at: Option<i64> = row.get(0);
            if committed_revoked_at == Some(revoked_at_unix_ms) {
                Ok(AnonymousCredentialPersistenceDisposition::Duplicate)
            } else {
                Err(AnonymousCredentialPersistenceError::ConflictingRevocation)
            }
        }
        (Some(_), Some(_)) => Err(AnonymousCredentialPersistenceError::ConflictingRevocation),
        (Some(_), None) => Err(AnonymousCredentialPersistenceError::ConflictingReplay),
    }
}

struct StoredCredentialEvidence<'a> {
    credential_ref: &'a str,
    tenant_ref: &'a str,
    participant_ref: &'a str,
    session_ref: &'a str,
    proof_digest: &'a str,
    issued_at_unix_ms: i64,
    expires_at_unix_ms: i64,
    revoked_at_unix_ms: Option<i64>,
}

fn credential_from_row(
    row: &postgres::Row,
) -> Result<AnonymousCredential, AnonymousCredentialPersistenceError> {
    let credential_ref: String = row.get(0);
    let tenant_ref: String = row.get(1);
    let participant_ref: String = row.get(2);
    let session_ref: String = row.get(3);
    let proof_digest: String = row.get(4);
    credential_from_stored(&StoredCredentialEvidence {
        credential_ref: &credential_ref,
        tenant_ref: &tenant_ref,
        participant_ref: &participant_ref,
        session_ref: &session_ref,
        proof_digest: &proof_digest,
        issued_at_unix_ms: row.get(5),
        expires_at_unix_ms: row.get(6),
        revoked_at_unix_ms: row.get(7),
    })
}

fn credential_from_stored(
    stored: &StoredCredentialEvidence<'_>,
) -> Result<AnonymousCredential, AnonymousCredentialPersistenceError> {
    let issued_at_unix_ms = u64::try_from(stored.issued_at_unix_ms)
        .map_err(|_| AnonymousCredentialPersistenceError::InvalidStoredEvidence)?;
    let expires_at_unix_ms = u64::try_from(stored.expires_at_unix_ms)
        .map_err(|_| AnonymousCredentialPersistenceError::InvalidStoredEvidence)?;
    let mut credential = AnonymousCredential::new(
        stored.credential_ref,
        stored.tenant_ref,
        stored.participant_ref,
        stored.session_ref,
        stored.proof_digest,
        issued_at_unix_ms,
        expires_at_unix_ms,
    )
    .map_err(|_| AnonymousCredentialPersistenceError::InvalidStoredEvidence)?;
    if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
        let revoked_at_unix_ms = u64::try_from(revoked_at_unix_ms)
            .map_err(|_| AnonymousCredentialPersistenceError::InvalidStoredEvidence)?;
        credential
            .revoke(revoked_at_unix_ms)
            .map_err(|_| AnonymousCredentialPersistenceError::InvalidStoredEvidence)?;
    }
    Ok(credential)
}

fn required_reference(reference: &str) -> Result<&str, AnonymousCredentialPersistenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AnonymousCredentialPersistenceError::InvalidReference),
    }
}

fn required_digest(digest: &str) -> Result<&str, AnonymousCredentialPersistenceError> {
    let hexadecimal = digest
        .strip_prefix("sha256:")
        .ok_or(AnonymousCredentialPersistenceError::InvalidDigest)?;
    if hexadecimal.len() == 64
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(digest)
    } else {
        Err(AnonymousCredentialPersistenceError::InvalidDigest)
    }
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, AnonymousCredentialPersistenceError> {
    i64::try_from(timestamp).map_err(|_| AnonymousCredentialPersistenceError::InvalidTimestamp)
}

fn is_unique_violation(error: &postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code == &SqlState::UNIQUE_VIOLATION)
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

#[cfg(test)]
mod tests {
    use super::{
        credential_from_stored, postgres_timestamp, required_digest, required_reference,
        AnonymousCredentialPersistenceError, StoredCredentialEvidence,
    };

    const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn stored_evidence(
        proof_digest: &str,
        issued_at_unix_ms: i64,
        expires_at_unix_ms: i64,
        revoked_at_unix_ms: Option<i64>,
    ) -> StoredCredentialEvidence<'_> {
        StoredCredentialEvidence {
            credential_ref: "anonymous_credential_alpha",
            tenant_ref: "tenant_alpha",
            participant_ref: "participant_alpha",
            session_ref: "session_alpha",
            proof_digest,
            issued_at_unix_ms,
            expires_at_unix_ms,
            revoked_at_unix_ms,
        }
    }

    #[test]
    fn reference_digest_timestamp_and_stored_rows_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(AnonymousCredentialPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(AnonymousCredentialPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("anonymous_credential_alpha").unwrap(),
            "anonymous_credential_alpha"
        );
        assert!(matches!(
            required_digest(
                "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            Err(AnonymousCredentialPersistenceError::InvalidDigest)
        ));
        assert!(matches!(
            required_digest("sha256:abc"),
            Err(AnonymousCredentialPersistenceError::InvalidDigest)
        ));
        assert!(matches!(
            required_digest(
                "sha256:gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg"
            ),
            Err(AnonymousCredentialPersistenceError::InvalidDigest)
        ));
        assert_eq!(
            required_digest(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .unwrap(),
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(AnonymousCredentialPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(postgres_timestamp(1_500).unwrap(), 1_500);
        assert!(matches!(
            credential_from_stored(&stored_evidence("sha256:not-a-digest", 1_000, 2_000, None)),
            Err(AnonymousCredentialPersistenceError::InvalidStoredEvidence)
        ));
        assert!(matches!(
            credential_from_stored(&stored_evidence(DIGEST, -1, 2_000, None)),
            Err(AnonymousCredentialPersistenceError::InvalidStoredEvidence)
        ));
        assert!(matches!(
            credential_from_stored(&stored_evidence(DIGEST, 1_000, -2, None)),
            Err(AnonymousCredentialPersistenceError::InvalidStoredEvidence)
        ));
        assert!(matches!(
            credential_from_stored(&stored_evidence(DIGEST, 1_000, 2_000, Some(-5))),
            Err(AnonymousCredentialPersistenceError::InvalidStoredEvidence)
        ));
        assert!(matches!(
            credential_from_stored(&stored_evidence(DIGEST, 1_000, 2_000, Some(0))),
            Err(AnonymousCredentialPersistenceError::InvalidStoredEvidence)
        ));
        let restored =
            credential_from_stored(&stored_evidence(DIGEST, 1_000, 2_000, Some(1_500))).unwrap();
        assert_eq!(restored.revoked_at_unix_ms(), Some(1_500));
        assert!(!restored.is_valid_at(1_500));
    }
}
