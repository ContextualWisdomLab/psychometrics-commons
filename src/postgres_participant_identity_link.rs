//! `PostgreSQL` 18 persistence for append-only participant identity-link history.
//!
//! Dual-proof account linking stays in the product domain. This adapter stores
//! the accepted history and a derived current-link projection so a restart can
//! reload the same participant identity. It does not parse Keyverse tokens or
//! rewrite historical participant, session, or result identifiers. Replay
//! classification requires `READ COMMITTED`.

use crate::participant::{AccountLinkEndEvent, AccountLinkEvent, ParticipantRecord};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const IDENTITY_LINK_MIGRATION: &str =
    include_str!("../migrations/0021_participant_identity_link.sql");

/// Outcome of persisting one participant identity-link history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IdentityLinkPersistenceDisposition {
    /// At least one new participant, link, or link-end row was inserted.
    Inserted,
    /// The same immutable participant and identity-link evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable participant identity-link persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum IdentityLinkPersistenceError {
    /// A participant, tenant, issuer, subject, event, or proof reference was invalid.
    InvalidReference,
    /// Event identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Identity-link persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// The issuer-scoped subject already has a current link on another participant.
    SubjectAlreadyBound,
    /// Stored history could not be replayed through the domain lifecycle.
    CorruptHistory,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for IdentityLinkPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "participant identity-link persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "participant identity-link evidence was replayed with conflicting values"
            }
            Self::InvalidTimestamp => {
                "participant identity-link timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "participant identity-link persistence requires read committed isolation"
            }
            Self::SubjectAlreadyBound => {
                "this issuer-scoped subject already has a current participant identity link"
            }
            Self::CorruptHistory => {
                "stored participant identity-link history could not be replayed"
            }
            Self::Database(_) => "PostgreSQL participant identity-link persistence failed",
        })
    }
}

impl Error for IdentityLinkPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidReference
            | Self::ConflictingReplay
            | Self::InvalidTimestamp
            | Self::UnsupportedIsolationLevel
            | Self::SubjectAlreadyBound
            | Self::CorruptHistory => None,
        }
    }
}

impl From<postgres::Error> for IdentityLinkPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent participant identity-link migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_participant_identity_link_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(IDENTITY_LINK_MIGRATION)
}

/// Persist one participant and its append-only identity-link history.
///
/// Exact replay of the same participant, link, and link-end evidence is
/// idempotent. Reusing an event identity with different issuer, subject, proof,
/// or time fails closed. A current issuer-scoped subject cannot belong to two
/// participants at once.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for unsupported isolation,
/// conflicting replay, an already-bound subject, an invalid reference, a
/// timestamp outside the `PostgreSQL` range, or a database failure.
pub fn persist_participant_identity_history(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<IdentityLinkPersistenceDisposition, IdentityLinkPersistenceError> {
    require_read_committed(transaction)?;
    let participant_ref = required_reference(participant.participant_ref())?;
    let tenant_ref = required_reference(participant.tenant_ref())?;
    let created_at = unix_ms_to_i64(participant.created_at_unix_ms())?;
    let mut inserted_any =
        persist_participant_header(transaction, participant_ref, tenant_ref, created_at)?;
    lock_participant(transaction, participant_ref)?;
    for event in participant.link_history() {
        if persist_one_link(transaction, participant_ref, tenant_ref, event)? {
            inserted_any = true;
        }
    }
    for event in participant.link_end_history() {
        if persist_one_link_end(transaction, participant_ref, event)? {
            inserted_any = true;
        }
    }
    if inserted_any {
        Ok(IdentityLinkPersistenceDisposition::Inserted)
    } else {
        Ok(IdentityLinkPersistenceDisposition::Duplicate)
    }
}

/// Reload one tenant-scoped participant and replay its identity-link history.
///
/// A missing participant or a tenant mismatch returns `None` so cross-tenant
/// probes cannot distinguish those cases. Loaded history is replayed through
/// [`ParticipantRecord`] so domain invariants remain authoritative.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for an invalid reference, corrupt
/// stored history, an unrepresentable timestamp, or a database failure.
pub fn load_participant_identity_history(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
) -> Result<Option<ParticipantRecord>, IdentityLinkPersistenceError> {
    let participant_ref = required_reference(participant_ref)?;
    let tenant_ref = required_reference(tenant_ref)?;
    let Some(created_at_unix_ms) =
        load_participant_header(transaction, participant_ref, tenant_ref)?
    else {
        return Ok(None);
    };
    let mut record =
        ParticipantRecord::new_anonymous(participant_ref, tenant_ref, created_at_unix_ms)
            .map_err(|_| IdentityLinkPersistenceError::CorruptHistory)?;
    let links = load_link_events(transaction, participant_ref)?;
    let ends = load_link_end_events(transaction, participant_ref)?;
    for link in &links {
        record
            .link_account(
                link.link_event_ref(),
                link.issuer_ref(),
                link.subject_ref(),
                link.anonymous_proof_ref(),
                link.authenticated_proof_ref(),
                link.linked_at_unix_ms(),
            )
            .map_err(|_| IdentityLinkPersistenceError::CorruptHistory)?;
        for end in ends
            .iter()
            .filter(|event| event.linked_event_ref() == link.link_event_ref())
        {
            record
                .record_link_end(
                    end.link_end_event_ref(),
                    end.evidence_ref(),
                    end.ended_at_unix_ms(),
                )
                .map_err(|_| IdentityLinkPersistenceError::CorruptHistory)?;
        }
    }
    if ends.iter().any(|end| {
        links
            .iter()
            .all(|link| link.link_event_ref() != end.linked_event_ref())
    }) {
        return Err(IdentityLinkPersistenceError::CorruptHistory);
    }
    Ok(Some(record))
}

fn persist_participant_header(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
    created_at_unix_ms: i64,
) -> Result<bool, IdentityLinkPersistenceError> {
    let inserted = transaction.execute(
        "INSERT INTO assessment_participant (\
             participant_ref, tenant_ref, created_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT (participant_ref) DO NOTHING",
        &[&participant_ref, &tenant_ref, &created_at_unix_ms],
    )?;
    if inserted == 1 {
        return Ok(true);
    }
    let row = transaction.query_one(
        "SELECT tenant_ref, created_at_unix_ms \
         FROM assessment_participant WHERE participant_ref = $1",
        &[&participant_ref],
    )?;
    let stored_tenant: String = row.get(0);
    let stored_created: i64 = row.get(1);
    if stored_tenant == tenant_ref && stored_created == created_at_unix_ms {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn lock_participant(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<(), IdentityLinkPersistenceError> {
    transaction.query_one(
        "SELECT participant_ref FROM assessment_participant \
         WHERE participant_ref = $1 FOR UPDATE",
        &[&participant_ref],
    )?;
    Ok(())
}

fn persist_one_link(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
    event: &AccountLinkEvent,
) -> Result<bool, IdentityLinkPersistenceError> {
    let identity_link_ref = required_reference(event.link_event_ref())?;
    let identity_issuer = required_reference(event.issuer_ref())?;
    let identity_subject_ref = required_reference(event.subject_ref())?;
    let anonymous_proof_ref = required_reference(event.anonymous_proof_ref())?;
    let authenticated_proof_ref = required_reference(event.authenticated_proof_ref())?;
    let linked_at_unix_ms = unix_ms_to_i64(event.linked_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO participant_identity_link (\
             identity_link_ref, participant_ref, tenant_ref, identity_issuer, \
             identity_subject_ref, anonymous_proof_ref, authenticated_proof_ref, \
             linked_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (identity_link_ref) DO NOTHING",
        &[
            &identity_link_ref,
            &participant_ref,
            &tenant_ref,
            &identity_issuer,
            &identity_subject_ref,
            &anonymous_proof_ref,
            &authenticated_proof_ref,
            &linked_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        insert_current_projection(
            transaction,
            participant_ref,
            identity_link_ref,
            tenant_ref,
            identity_issuer,
            identity_subject_ref,
        )?;
        return Ok(true);
    }
    let row = transaction.query_one(
        "SELECT participant_ref, tenant_ref, identity_issuer, identity_subject_ref, \
                anonymous_proof_ref, authenticated_proof_ref, linked_at_unix_ms \
         FROM participant_identity_link WHERE identity_link_ref = $1",
        &[&identity_link_ref],
    )?;
    let stored_participant: String = row.get(0);
    let stored_tenant: String = row.get(1);
    let stored_issuer: String = row.get(2);
    let stored_subject: String = row.get(3);
    let stored_anonymous: String = row.get(4);
    let stored_authenticated: String = row.get(5);
    let stored_linked: i64 = row.get(6);
    if stored_participant == participant_ref
        && stored_tenant == tenant_ref
        && stored_issuer == identity_issuer
        && stored_subject == identity_subject_ref
        && stored_anonymous == anonymous_proof_ref
        && stored_authenticated == authenticated_proof_ref
        && stored_linked == linked_at_unix_ms
    {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn insert_current_projection(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    identity_link_ref: &str,
    tenant_ref: &str,
    identity_issuer: &str,
    identity_subject_ref: &str,
) -> Result<(), IdentityLinkPersistenceError> {
    match transaction.execute(
        "INSERT INTO current_participant_identity_link (\
             participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
             identity_subject_ref\
         ) VALUES ($1, $2, $3, $4, $5)",
        &[
            &participant_ref,
            &identity_link_ref,
            &tenant_ref,
            &identity_issuer,
            &identity_subject_ref,
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error) => Err(classify_current_unique_violation(error)),
    }
}

fn persist_one_link_end(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    event: &AccountLinkEndEvent,
) -> Result<bool, IdentityLinkPersistenceError> {
    let link_end_event_ref = required_reference(event.link_end_event_ref())?;
    let linked_event_ref = required_reference(event.linked_event_ref())?;
    let evidence_ref = required_reference(event.evidence_ref())?;
    let ended_at_unix_ms = unix_ms_to_i64(event.ended_at_unix_ms())?;
    let inserted = transaction.execute(
        "INSERT INTO participant_identity_link_end (\
             link_end_event_ref, participant_ref, linked_event_ref, evidence_ref, \
             ended_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (link_end_event_ref) DO NOTHING",
        &[
            &link_end_event_ref,
            &participant_ref,
            &linked_event_ref,
            &evidence_ref,
            &ended_at_unix_ms,
        ],
    )?;
    if inserted == 1 {
        transaction.execute(
            "DELETE FROM current_participant_identity_link \
             WHERE participant_ref = $1 AND identity_link_ref = $2",
            &[&participant_ref, &linked_event_ref],
        )?;
        return Ok(true);
    }
    let row = transaction.query_one(
        "SELECT participant_ref, linked_event_ref, evidence_ref, ended_at_unix_ms \
         FROM participant_identity_link_end WHERE link_end_event_ref = $1",
        &[&link_end_event_ref],
    )?;
    let stored_participant: String = row.get(0);
    let stored_linked: String = row.get(1);
    let stored_evidence: String = row.get(2);
    let stored_ended: i64 = row.get(3);
    if stored_participant == participant_ref
        && stored_linked == linked_event_ref
        && stored_evidence == evidence_ref
        && stored_ended == ended_at_unix_ms
    {
        Ok(false)
    } else {
        Err(IdentityLinkPersistenceError::ConflictingReplay)
    }
}

fn load_participant_header(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
) -> Result<Option<u64>, IdentityLinkPersistenceError> {
    let row = transaction.query_opt(
        "SELECT created_at_unix_ms FROM assessment_participant \
         WHERE participant_ref = $1 AND tenant_ref = $2",
        &[&participant_ref, &tenant_ref],
    )?;
    row.map(|row| i64_to_unix_ms(row.get(0))).transpose()
}

fn load_link_events(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<Vec<AccountLinkEvent>, IdentityLinkPersistenceError> {
    let rows = transaction.query(
        "SELECT identity_link_ref, identity_issuer, identity_subject_ref, \
                anonymous_proof_ref, authenticated_proof_ref, linked_at_unix_ms \
         FROM participant_identity_link \
         WHERE participant_ref = $1 \
         ORDER BY linked_at_unix_ms, identity_link_ref",
        &[&participant_ref],
    )?;
    let mut events = Vec::new();
    for row in rows {
        events.push(AccountLinkEvent::from_stored(
            row.get(0),
            row.get(1),
            row.get(2),
            row.get(3),
            row.get(4),
            i64_to_unix_ms(row.get(5))?,
        ));
    }
    Ok(events)
}

fn load_link_end_events(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
) -> Result<Vec<AccountLinkEndEvent>, IdentityLinkPersistenceError> {
    let rows = transaction.query(
        "SELECT link_end_event_ref, linked_event_ref, evidence_ref, ended_at_unix_ms \
         FROM participant_identity_link_end \
         WHERE participant_ref = $1 \
         ORDER BY ended_at_unix_ms, link_end_event_ref",
        &[&participant_ref],
    )?;
    let mut events = Vec::new();
    for row in rows {
        events.push(AccountLinkEndEvent::from_stored(
            row.get(0),
            row.get(1),
            row.get(2),
            i64_to_unix_ms(row.get(3))?,
        ));
    }
    Ok(events)
}

fn classify_current_unique_violation(error: postgres::Error) -> IdentityLinkPersistenceError {
    match error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
    {
        Some("current_participant_identity_link_subject_unique") => {
            IdentityLinkPersistenceError::SubjectAlreadyBound
        }
        Some("current_participant_identity_link_pkey") => {
            IdentityLinkPersistenceError::ConflictingReplay
        }
        _ => IdentityLinkPersistenceError::Database(error),
    }
}

fn required_reference(reference: &str) -> Result<&str, IdentityLinkPersistenceError> {
    normalized_reference(reference).ok_or(IdentityLinkPersistenceError::InvalidReference)
}

fn unix_ms_to_i64(value: u64) -> Result<i64, IdentityLinkPersistenceError> {
    i64::try_from(value).map_err(|_| IdentityLinkPersistenceError::InvalidTimestamp)
}

fn i64_to_unix_ms(value: i64) -> Result<u64, IdentityLinkPersistenceError> {
    u64::try_from(value).map_err(|_| IdentityLinkPersistenceError::InvalidTimestamp)
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
mod tests {
    use super::{required_reference, unix_ms_to_i64, IdentityLinkPersistenceError};

    #[test]
    fn blank_and_numeric_references_fail_closed() {
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
    }

    #[test]
    fn persistence_errors_expose_stable_operator_messages() {
        for (error, expected) in [
            (
                IdentityLinkPersistenceError::InvalidReference,
                "participant identity-link persistence references must be opaque values",
            ),
            (
                IdentityLinkPersistenceError::ConflictingReplay,
                "participant identity-link evidence was replayed with conflicting values",
            ),
            (
                IdentityLinkPersistenceError::InvalidTimestamp,
                "participant identity-link timestamp exceeds the PostgreSQL bigint range",
            ),
            (
                IdentityLinkPersistenceError::UnsupportedIsolationLevel,
                "participant identity-link persistence requires read committed isolation",
            ),
            (
                IdentityLinkPersistenceError::SubjectAlreadyBound,
                "this issuer-scoped subject already has a current participant identity link",
            ),
            (
                IdentityLinkPersistenceError::CorruptHistory,
                "stored participant identity-link history could not be replayed",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn timestamps_outside_signed_bigint_fail_closed() {
        assert!(matches!(
            unix_ms_to_i64(u64::MAX),
            Err(IdentityLinkPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(unix_ms_to_i64(10_100).unwrap(), 10_100);
    }
}
