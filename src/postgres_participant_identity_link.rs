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
    include_str!("../migrations/0022_participant_identity_link.sql");

/// Count of derived current-projection rows that do not match unterminated history.
///
/// After a dump restore, operators inspect this before accepting new account-link
/// writes. History remains the lookup source of truth; this count tells the
/// operator to run [`reconcile_identity_link_current_projections`] so the unique
/// enforcer matches that history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityLinkProjectionDrift {
    missing_current_rows: u64,
    stale_current_rows: u64,
}

impl IdentityLinkProjectionDrift {
    /// Number of unterminated history rows that have no matching current projection.
    #[must_use]
    pub const fn missing_current_rows(self) -> u64 {
        self.missing_current_rows
    }

    /// Number of current-projection rows that do not match unterminated history.
    #[must_use]
    pub const fn stale_current_rows(self) -> u64 {
        self.stale_current_rows
    }

    /// Return whether the unique enforcer is missing or stale after restore.
    #[must_use]
    pub const fn has_drift(self) -> bool {
        self.missing_current_rows > 0 || self.stale_current_rows > 0
    }

    /// Return whether new account-link writes may use the derived unique enforcer.
    ///
    /// A returning login can still resolve from unterminated history. New first
    /// inserts must wait until restore reconcile rebuilds the unique constraint.
    #[must_use]
    pub const fn accepts_new_account_link_writes(self) -> bool {
        !self.has_drift()
    }
}

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
/// History is applied in the same lifecycle order as reload: each link, then
/// the ends that close that link. Exact replay of the same participant, link,
/// and link-end evidence is idempotent. After that history is written, the
/// derived current projection is reconciled so operator repair or restore
/// cannot leave a missing or stale unique enforcer. Reusing an event identity
/// with different issuer, subject, proof, or time fails closed. An
/// unterminated issuer-scoped subject cannot belong to two participants at
/// once, even when the derived current projection is missing.
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
    if participant.link_end_history().iter().any(|end| {
        participant
            .link_history()
            .iter()
            .all(|link| link.link_event_ref() != end.linked_event_ref())
    }) {
        return Err(IdentityLinkPersistenceError::CorruptHistory);
    }
    for event in participant.link_history() {
        if persist_one_link(transaction, participant_ref, tenant_ref, event)? {
            inserted_any = true;
        }
        for end in participant
            .link_end_history()
            .iter()
            .filter(|end| end.linked_event_ref() == event.link_event_ref())
        {
            if persist_one_link_end(transaction, participant_ref, end)? {
                inserted_any = true;
            }
        }
    }
    reconcile_current_projection(transaction, participant)?;
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

/// Reload the participant that currently holds an issuer-scoped subject.
///
/// A returning Keyverse login uses this lookup to recover the stable
/// product-owned `participant_ref` after the anonymous session token is gone.
/// The append-only link history is the source of truth: a derived current
/// projection may be missing after restore or operator repair, but an
/// unterminated issuer-scoped subject still resolves. A missing current link
/// or a tenant mismatch returns `None`.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for an invalid reference, corrupt
/// stored history, an unrepresentable timestamp, or a database failure.
pub fn load_participant_by_current_identity_subject(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    identity_issuer: &str,
    identity_subject_ref: &str,
) -> Result<Option<ParticipantRecord>, IdentityLinkPersistenceError> {
    let tenant_ref = required_reference(tenant_ref)?;
    let identity_issuer = required_reference(identity_issuer)?;
    let identity_subject_ref = required_reference(identity_subject_ref)?;
    let Some(participant_ref) = current_subject_participant(
        transaction,
        tenant_ref,
        identity_issuer,
        identity_subject_ref,
    )?
    else {
        return Ok(None);
    };
    load_participant_identity_history(transaction, &participant_ref, tenant_ref)
}

/// Rebuild every derived current identity-link projection from unterminated history.
///
/// After a backup restore or operator repair, the unique enforcer may be
/// missing or stale even though append-only history is intact. A returning
/// login already resolves from that history. Run this before accepting new
/// account-link writes so concurrent first-inserts cannot bind a subject that
/// already has an unterminated holder. Two unterminated holders of the same
/// issuer-scoped subject, or two current links on one participant, fail closed.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for unsupported isolation, corrupt
/// unterminated history, or a database failure.
pub fn reconcile_identity_link_current_projections(
    transaction: &mut Transaction<'_>,
) -> Result<u64, IdentityLinkPersistenceError> {
    require_read_committed(transaction)?;
    transaction.query(
        "SELECT participant_ref FROM assessment_participant \
         ORDER BY participant_ref FOR UPDATE",
        &[],
    )?;
    reject_corrupt_unterminated_history(transaction)?;
    transaction.execute("DELETE FROM current_participant_identity_link", &[])?;
    let inserted = transaction.execute(
        "INSERT INTO current_participant_identity_link (\
             participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
             identity_subject_ref\
         ) \
         SELECT l.participant_ref, l.identity_link_ref, l.tenant_ref, \
                l.identity_issuer, l.identity_subject_ref \
         FROM participant_identity_link l \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM participant_identity_link_end e \
             WHERE e.linked_event_ref = l.identity_link_ref \
         )",
        &[],
    )?;
    Ok(inserted)
}

/// Inspect whether the derived current projection matches unterminated history.
///
/// This does not mutate rows. After restore, a missing or stale unique enforcer
/// means operators must run [`reconcile_identity_link_current_projections`]
/// before accepting new account-link writes. Two unterminated holders of the
/// same issuer-scoped subject, or two current links on one participant, fail
/// closed.
///
/// # Errors
///
/// Returns [`IdentityLinkPersistenceError`] for unsupported isolation, corrupt
/// unterminated history, or a database failure.
pub fn inspect_identity_link_current_projection_drift(
    transaction: &mut Transaction<'_>,
) -> Result<IdentityLinkProjectionDrift, IdentityLinkPersistenceError> {
    require_read_committed(transaction)?;
    reject_corrupt_unterminated_history(transaction)?;
    Ok(IdentityLinkProjectionDrift {
        missing_current_rows: count_sql_rows(
            transaction,
            "SELECT COUNT(*)::bigint FROM participant_identity_link history_link \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM participant_identity_link_end link_end \
                 WHERE link_end.linked_event_ref = history_link.identity_link_ref \
             ) \
             AND NOT EXISTS ( \
                 SELECT 1 FROM current_participant_identity_link current_link \
                 WHERE current_link.participant_ref = history_link.participant_ref \
                   AND current_link.identity_link_ref = history_link.identity_link_ref \
                   AND current_link.tenant_ref = history_link.tenant_ref \
                   AND current_link.identity_issuer = history_link.identity_issuer \
                   AND current_link.identity_subject_ref = history_link.identity_subject_ref \
             )",
        )?,
        stale_current_rows: count_sql_rows(
            transaction,
            "SELECT COUNT(*)::bigint FROM current_participant_identity_link current_link \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM participant_identity_link history_link \
                 WHERE history_link.participant_ref = current_link.participant_ref \
                   AND history_link.identity_link_ref = current_link.identity_link_ref \
                   AND history_link.tenant_ref = current_link.tenant_ref \
                   AND history_link.identity_issuer = current_link.identity_issuer \
                   AND history_link.identity_subject_ref = current_link.identity_subject_ref \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM participant_identity_link_end link_end \
                       WHERE link_end.linked_event_ref = history_link.identity_link_ref \
                   ) \
             )",
        )?,
    })
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
    reject_subject_bound_to_another_participant(
        transaction,
        participant_ref,
        tenant_ref,
        identity_issuer,
        identity_subject_ref,
    )?;
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

fn current_link_event(participant: &ParticipantRecord) -> Option<&AccountLinkEvent> {
    participant.link_history().iter().rev().find(|link| {
        participant
            .link_end_history()
            .iter()
            .all(|end| end.linked_event_ref() != link.link_event_ref())
    })
}

fn reconcile_current_projection(
    transaction: &mut Transaction<'_>,
    participant: &ParticipantRecord,
) -> Result<(), IdentityLinkPersistenceError> {
    let participant_ref = required_reference(participant.participant_ref())?;
    if let Some(event) = current_link_event(participant) {
        let tenant_ref = required_reference(participant.tenant_ref())?;
        let identity_link_ref = required_reference(event.link_event_ref())?;
        let identity_issuer = required_reference(event.issuer_ref())?;
        let identity_subject_ref = required_reference(event.subject_ref())?;
        match transaction.execute(
            "INSERT INTO current_participant_identity_link (\
                 participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref\
             ) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (participant_ref) DO UPDATE SET \
                 identity_link_ref = EXCLUDED.identity_link_ref, \
                 tenant_ref = EXCLUDED.tenant_ref, \
                 identity_issuer = EXCLUDED.identity_issuer, \
                 identity_subject_ref = EXCLUDED.identity_subject_ref",
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
    } else {
        transaction.execute(
            "DELETE FROM current_participant_identity_link WHERE participant_ref = $1",
            &[&participant_ref],
        )?;
        Ok(())
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
         WHERE participant_ref = $1 AND tenant_ref = $2 FOR SHARE",
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

fn current_subject_participant(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    identity_issuer: &str,
    identity_subject_ref: &str,
) -> Result<Option<String>, IdentityLinkPersistenceError> {
    let rows = transaction.query(
        "SELECT l.participant_ref \
         FROM participant_identity_link l \
         WHERE l.tenant_ref = $1 \
           AND l.identity_issuer = $2 \
           AND l.identity_subject_ref = $3 \
           AND NOT EXISTS ( \
               SELECT 1 FROM participant_identity_link_end e \
               WHERE e.linked_event_ref = l.identity_link_ref \
           ) \
         FOR SHARE",
        &[&tenant_ref, &identity_issuer, &identity_subject_ref],
    )?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => Ok(Some(row.get(0))),
        _ => Err(IdentityLinkPersistenceError::CorruptHistory),
    }
}

fn reject_corrupt_unterminated_history(
    transaction: &mut Transaction<'_>,
) -> Result<(), IdentityLinkPersistenceError> {
    let duplicate_subjects: bool = transaction
        .query_one(
            "SELECT EXISTS ( \
                 SELECT 1 \
                 FROM participant_identity_link current_link \
                 WHERE NOT EXISTS ( \
                     SELECT 1 FROM participant_identity_link_end link_end \
                     WHERE link_end.linked_event_ref = current_link.identity_link_ref \
                 ) \
                 GROUP BY current_link.tenant_ref, current_link.identity_issuer, \
                          current_link.identity_subject_ref \
                 HAVING COUNT(*) > 1 \
             )",
            &[],
        )?
        .get(0);
    let duplicate_participants: bool = transaction
        .query_one(
            "SELECT EXISTS ( \
                 SELECT 1 \
                 FROM participant_identity_link current_link \
                 WHERE NOT EXISTS ( \
                     SELECT 1 FROM participant_identity_link_end link_end \
                     WHERE link_end.linked_event_ref = current_link.identity_link_ref \
                 ) \
                 GROUP BY current_link.participant_ref \
                 HAVING COUNT(*) > 1 \
             )",
            &[],
        )?
        .get(0);
    if duplicate_subjects || duplicate_participants {
        Err(IdentityLinkPersistenceError::CorruptHistory)
    } else {
        Ok(())
    }
}

fn reject_subject_bound_to_another_participant(
    transaction: &mut Transaction<'_>,
    participant_ref: &str,
    tenant_ref: &str,
    identity_issuer: &str,
    identity_subject_ref: &str,
) -> Result<(), IdentityLinkPersistenceError> {
    match current_subject_participant(
        transaction,
        tenant_ref,
        identity_issuer,
        identity_subject_ref,
    )? {
        Some(holder) if holder != participant_ref => {
            Err(IdentityLinkPersistenceError::SubjectAlreadyBound)
        }
        Some(_) | None => Ok(()),
    }
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

fn count_sql_rows(
    transaction: &mut Transaction<'_>,
    sql: &str,
) -> Result<u64, IdentityLinkPersistenceError> {
    let count: i64 = transaction.query_one(sql, &[])?.get(0);
    u64::try_from(count).map_err(|_| IdentityLinkPersistenceError::CorruptHistory)
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
    use super::{
        required_reference, unix_ms_to_i64, IdentityLinkPersistenceError,
        IdentityLinkProjectionDrift,
    };

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
    fn projection_drift_blocks_new_writes_until_reconcile() {
        let drifted = IdentityLinkProjectionDrift {
            missing_current_rows: 2,
            stale_current_rows: 1,
        };
        assert_eq!(drifted.missing_current_rows(), 2);
        assert_eq!(drifted.stale_current_rows(), 1);
        assert!(drifted.has_drift());
        assert!(!drifted.accepts_new_account_link_writes());

        let reconciled = IdentityLinkProjectionDrift {
            missing_current_rows: 0,
            stale_current_rows: 0,
        };
        assert!(!reconciled.has_drift());
        assert!(reconciled.accepts_new_account_link_writes());
    }

    #[test]
    fn timestamps_outside_signed_bigint_fail_closed() {
        assert!(matches!(
            unix_ms_to_i64(u64::MAX),
            Err(IdentityLinkPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(unix_ms_to_i64(10_100).unwrap(), 10_100);
        assert!(matches!(
            super::i64_to_unix_ms(-1),
            Err(IdentityLinkPersistenceError::InvalidTimestamp)
        ));
        assert_eq!(super::i64_to_unix_ms(10_100).unwrap(), 10_100);
    }
}
