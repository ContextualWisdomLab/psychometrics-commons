//! `PostgreSQL` 18 persistence for append-only participant identity-link history.
//!
//! This adapter stores product-owned participant identity and Keyverse proof
//! references only. Credentials remain in Keyverse. The caller owns the
//! connection, credentials, and transaction boundary. Replay requires
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is visible
//! to the exact-replay classifier.

use crate::participant::{AccountLinkEndEvent, AccountLinkEvent, ParticipantRecord};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const IDENTITY_LINK_MIGRATION: &str =
    include_str!("../migrations/0008_participant_identity_link.sql");

/// Outcome of persisting one participant identity-link snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IdentityLinkPersistenceDisposition {
    /// At least one new ledger, link, or link-end row was inserted.
    Inserted,
    /// The same immutable ledger and event evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable identity-link persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum IdentityLinkPersistenceError {
    /// A participant, tenant, or event identity was blank or numeric-like.
    InvalidReference,
    /// Event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Identity-link persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for IdentityLinkPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "identity-link persistence references must be opaque values",
            Self::ConflictingReplay => {
                "identity-link identity was replayed with conflicting evidence"
            }
            Self::InvalidTimestamp => "identity-link timestamp exceeds the PostgreSQL bigint range",
            Self::UnsupportedIsolationLevel => {
                "identity-link persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL identity-link persistence failed",
        })
    }
}

impl Error for IdentityLinkPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for IdentityLinkPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent identity-link migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_identity_link_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(IDENTITY_LINK_MIGRATION)
}

/// Persist one participant ledger and its accepted identity-link history.
///
/// Exact replay of the same participant, link, and link-end evidence is
/// idempotent. Reusing an event identity with different issuer, subject, proof,
/// or time fails closed. New events append without rewriting earlier evidence.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for unsupported isolation,
/// conflicting replay, an invalid reference or timestamp, or a database failure.
pub fn persist_participant_identity(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<IdentityLinkPersistenceDisposition, IdentityLinkPersistenceError> {
    require_read_committed(transaction)?;
    let participant_ref = required_reference(participant.participant_ref())?;
    let mut inserted_any = persist_ledger_header(transaction, participant)?;
    for event in participant.link_history() {
        if persist_one_link_event(transaction, participant_ref, event)? {
            inserted_any = true;
        }
    }
    for event in participant.link_end_history() {
        if persist_one_end_event(transaction, participant_ref, event)? {
            inserted_any = true;
        }
    }
    if inserted_any {
        Ok(IdentityLinkPersistenceDisposition::Inserted)
    } else {
        Ok(IdentityLinkPersistenceDisposition::Duplicate)
    }
}

fn persist_ledger_header(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<bool, IdentityLinkPersistenceError> {
    let created_at = postgres_timestamp(participant.created_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO participant_identity_ledger (\
             participant_ref, tenant_ref, created_at_unix_ms\
         ) VALUES ($1, $2, $3) ON CONFLICT (participant_ref) DO NOTHING",
        &[
            &participant.participant_ref(),
            &participant.tenant_ref(),
            &created_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT tenant_ref, created_at_unix_ms FROM participant_identity_ledger \
         WHERE participant_ref = $1",
        &[&participant.participant_ref()],
    )?;
    let stored_tenant: String = row.get(0);
    let stored_created: i64 = row.get(1);
    if stored_tenant == participant.tenant_ref() && stored_created == created_at {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn persist_one_link_event(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    event: &AccountLinkEvent,
) -> Result<bool, IdentityLinkPersistenceError> {
    let event_ref = event.link_event_ref();
    let linked_at = postgres_timestamp(event.linked_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO participant_identity_link_event (\
             participant_ref, link_event_ref, issuer_ref, subject_ref, \
             anonymous_proof_ref, authenticated_proof_ref, linked_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (participant_ref, link_event_ref) DO NOTHING",
        &[
            &participant_ref,
            &event_ref,
            &event.issuer_ref(),
            &event.subject_ref(),
            &event.anonymous_proof_ref(),
            &event.authenticated_proof_ref(),
            &linked_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT issuer_ref, subject_ref, anonymous_proof_ref, authenticated_proof_ref, \
                linked_at_unix_ms \
         FROM participant_identity_link_event \
         WHERE participant_ref = $1 AND link_event_ref = $2",
        &[&participant_ref, &event_ref],
    )?;
    let stored_issuer: String = row.get(0);
    let stored_subject: String = row.get(1);
    let stored_anonymous: String = row.get(2);
    let stored_authenticated: String = row.get(3);
    let stored_linked: i64 = row.get(4);
    if stored_issuer == event.issuer_ref()
        && stored_subject == event.subject_ref()
        && stored_anonymous == event.anonymous_proof_ref()
        && stored_authenticated == event.authenticated_proof_ref()
        && stored_linked == linked_at
    {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn persist_one_end_event(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    event: &AccountLinkEndEvent,
) -> Result<bool, IdentityLinkPersistenceError> {
    let event_ref = event.link_end_event_ref();
    let ended_at = postgres_timestamp(event.ended_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO participant_identity_link_end_event (\
             participant_ref, link_end_event_ref, linked_event_ref, evidence_ref, \
             ended_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (participant_ref, link_end_event_ref) DO NOTHING",
        &[
            &participant_ref,
            &event_ref,
            &event.linked_event_ref(),
            &event.evidence_ref(),
            &ended_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(true);
    }

    let row = transaction.query_one(
        "SELECT linked_event_ref, evidence_ref, ended_at_unix_ms \
         FROM participant_identity_link_end_event \
         WHERE participant_ref = $1 AND link_end_event_ref = $2",
        &[&participant_ref, &event_ref],
    )?;
    let stored_linked: String = row.get(0);
    let stored_evidence: String = row.get(1);
    let stored_ended: i64 = row.get(2);
    if stored_linked == event.linked_event_ref()
        && stored_evidence == event.evidence_ref()
        && stored_ended == ended_at
    {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn required_reference(reference: &str) -> Result<&str, IdentityLinkPersistenceError> {
    normalized_reference(reference).ok_or(IdentityLinkPersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, IdentityLinkPersistenceError> {
    i64::try_from(timestamp).map_err(|_| IdentityLinkPersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), IdentityLinkPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(IdentityLinkPersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{postgres_timestamp, required_reference, IdentityLinkPersistenceError};

    #[test]
    fn blank_numeric_and_overflow_inputs_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(IdentityLinkPersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(IdentityLinkPersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("participant_identity_alpha").unwrap(),
            "participant_identity_alpha"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(IdentityLinkPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(postgres_timestamp(10_000).unwrap(), 10_000);
    }
}
