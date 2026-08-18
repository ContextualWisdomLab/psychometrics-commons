//! `PostgreSQL` 18 persist and reload for live measurement sessions.
//!
//! The adapter stores participants, consent records, audit events, and an export
//! snapshot pointer. Consent and audit payloads are AES-256-GCM sealed with a
//! purpose-bound key. Numeric scores, IRT, linking, and identity-link history
//! are not persisted here.

use crate::authorization::{AuthorizationContext, AuthorizationError};
use crate::measurement_session::{
    authorize_measurement_session, authorize_stored_measurement_session, ExportSnapshotPointer,
    MeasurementSession, MeasurementSessionError, MeasurementSessionInput, SealedPayload,
    SessionAuditEvent, SessionConsentRecord, SessionEncryptionKey, SessionMembership,
};
use crate::reference::normalized_reference;
use postgres::types::ToSql;
use postgres::{Row, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const MEASUREMENT_SESSION_MIGRATION: &str =
    include_str!("../migrations/0020_measurement_session.sql");

/// Outcome of persisting one live measurement session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MeasurementSessionPersistenceDisposition {
    /// At least one new participant, session, membership, consent, audit, or pointer row was inserted.
    Inserted,
    /// The same immutable session evidence already existed.
    Duplicate,
}

/// Fail-closed error for durable measurement-session persist and reload.
#[derive(Debug)]
#[non_exhaustive]
pub enum MeasurementSessionPersistenceError {
    /// A session, tenant, participant, event, or snapshot reference was invalid.
    InvalidReference,
    /// Session identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// A timestamp cannot be represented by the bounded database column.
    ValueOutOfRange,
    /// Persist and reload require `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// The actor is not authorized for this persist or reload purpose.
    Unauthorized(AuthorizationError),
    /// Domain construction or sealing failed while reading stored evidence.
    Domain(MeasurementSessionError),
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for MeasurementSessionPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "measurement session persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "measurement session identity was replayed with conflicting evidence"
            }
            Self::ValueOutOfRange => {
                "measurement session timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "measurement session persist and reload require read committed isolation"
            }
            Self::Unauthorized(_) => "measurement session persist and reload are unauthorized",
            Self::Domain(_) => "stored measurement session evidence could not be restored",
            Self::Database(_) => "PostgreSQL measurement session persistence failed",
        })
    }
}

impl Error for MeasurementSessionPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unauthorized(error) => Some(error),
            Self::Domain(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::InvalidReference
            | Self::ConflictingReplay
            | Self::ValueOutOfRange
            | Self::UnsupportedIsolationLevel => None,
        }
    }
}

impl From<postgres::Error> for MeasurementSessionPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

impl From<AuthorizationError> for MeasurementSessionPersistenceError {
    fn from(error: AuthorizationError) -> Self {
        Self::Unauthorized(error)
    }
}

impl From<MeasurementSessionError> for MeasurementSessionPersistenceError {
    fn from(error: MeasurementSessionError) -> Self {
        match error {
            MeasurementSessionError::InvalidReference => Self::InvalidReference,
            MeasurementSessionError::InvalidTimestamp => Self::ValueOutOfRange,
            other => Self::Domain(other),
        }
    }
}

/// Apply the idempotent measurement-session migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_measurement_session_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(MEASUREMENT_SESSION_MIGRATION)
}

/// Persist one live measurement session under purpose-limited authorization.
///
/// Exact replay of the same participants, consent, audit trail, and export
/// pointer is idempotent. Rebinding any stored field fails closed. Consent and
/// audit payloads are sealed with the caller-supplied purpose-bound key.
///
/// # Errors
///
/// Returns [`MeasurementSessionPersistenceError`] for unauthorized callers,
/// unsupported isolation, conflicting replay, invalid evidence, or a database
/// failure.
pub fn persist_measurement_session(
    transaction: &mut Transaction<'_>,
    actor: &AuthorizationContext,
    session: &MeasurementSession,
    encryption_key: &SessionEncryptionKey,
) -> Result<MeasurementSessionPersistenceDisposition, MeasurementSessionPersistenceError> {
    require_read_committed(transaction)?;
    authorize_measurement_session(actor, session)?;
    let mut inserted_any = false;
    for membership in session.memberships() {
        if persist_participant(transaction, membership)? {
            inserted_any = true;
        }
    }
    if persist_session_header(transaction, session)? {
        inserted_any = true;
    }
    for membership in session.memberships() {
        if persist_membership(transaction, session.session_ref(), membership)? {
            inserted_any = true;
        }
    }
    for record in session.consent_records() {
        if persist_consent_record(transaction, session.session_ref(), record, encryption_key)? {
            inserted_any = true;
        }
    }
    for event in session.audit_events() {
        if persist_audit_event(transaction, session.session_ref(), event, encryption_key)? {
            inserted_any = true;
        }
    }
    if persist_export_pointer(
        transaction,
        session.session_ref(),
        session.export_snapshot_pointer(),
    )? {
        inserted_any = true;
    }
    if inserted_any {
        Ok(MeasurementSessionPersistenceDisposition::Inserted)
    } else {
        Ok(MeasurementSessionPersistenceDisposition::Duplicate)
    }
}

/// Reload one live measurement session after process death or a later request.
///
/// Authorization uses the stored tenant and owner. Ciphertext is opened only
/// after that check succeeds. Missing sessions return [`None`].
///
/// # Errors
///
/// Returns [`MeasurementSessionPersistenceError`] for unauthorized callers,
/// unsupported isolation, corrupt stored evidence, a wrong key, or a database
/// failure.
pub fn load_measurement_session(
    transaction: &mut Transaction<'_>,
    actor: &AuthorizationContext,
    session_ref: &str,
    encryption_key: &SessionEncryptionKey,
) -> Result<Option<MeasurementSession>, MeasurementSessionPersistenceError> {
    require_read_committed(transaction)?;
    let session_ref = required_reference(session_ref)?;
    let Some(header) = load_session_header(transaction, session_ref)? else {
        return Ok(None);
    };
    authorize_stored_measurement_session(
        actor,
        &header.tenant_ref,
        &header.owner_participant_ref,
        session_ref,
    )?;
    let memberships = load_memberships(transaction, session_ref)?;
    let consent_records = load_consent_records(transaction, session_ref, encryption_key)?;
    let audit_events = load_audit_events(transaction, session_ref, encryption_key)?;
    let export_snapshot_pointer = load_export_pointer(transaction, session_ref)?;
    Ok(Some(MeasurementSession::new(MeasurementSessionInput {
        session_ref: session_ref.to_owned(),
        tenant_ref: header.tenant_ref,
        owner_participant_ref: header.owner_participant_ref,
        created_at_unix_ms: header.created_at_unix_ms,
        memberships,
        consent_records,
        audit_events,
        export_snapshot_pointer,
    })?))
}

struct StoredSessionHeader {
    tenant_ref: String,
    owner_participant_ref: String,
    created_at_unix_ms: u64,
}

fn persist_participant(
    transaction: &mut Transaction<'_>,
    membership: &SessionMembership,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let created_at = unix_ms(membership.created_at_unix_ms())?;
    insert_then_classify(
        transaction,
        "INSERT INTO assessment_participant (\
             participant_ref, tenant_ref, created_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT (participant_ref) DO NOTHING",
        &[
            &membership.participant_ref(),
            &membership.tenant_ref(),
            &created_at,
        ],
        "SELECT tenant_ref, created_at_unix_ms \
         FROM assessment_participant WHERE participant_ref = $1",
        &[&membership.participant_ref()],
        &|row| {
            let stored_tenant: String = row.get(0);
            let stored_created: i64 = row.get(1);
            stored_tenant == membership.tenant_ref() && stored_created == created_at
        },
    )
}

fn persist_session_header(
    transaction: &mut Transaction<'_>,
    session: &MeasurementSession,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let created_at = unix_ms(session.created_at_unix_ms())?;
    insert_then_classify(
        transaction,
        "INSERT INTO measurement_session (\
             session_ref, tenant_ref, owner_participant_ref, created_at_unix_ms\
         ) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (session_ref) DO NOTHING",
        &[
            &session.session_ref(),
            &session.tenant_ref(),
            &session.owner_participant_ref(),
            &created_at,
        ],
        "SELECT tenant_ref, owner_participant_ref, created_at_unix_ms \
         FROM measurement_session WHERE session_ref = $1",
        &[&session.session_ref()],
        &|row| {
            let stored_tenant: String = row.get(0);
            let stored_owner: String = row.get(1);
            let stored_created: i64 = row.get(2);
            stored_tenant == session.tenant_ref()
                && stored_owner == session.owner_participant_ref()
                && stored_created == created_at
        },
    )
}

fn persist_membership(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    membership: &SessionMembership,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let enrolled_at = unix_ms(membership.enrolled_at_unix_ms())?;
    insert_then_classify(
        transaction,
        "INSERT INTO session_membership (\
             session_ref, participant_ref, enrolled_at_unix_ms\
         ) VALUES ($1, $2, $3) \
         ON CONFLICT (session_ref, participant_ref) DO NOTHING",
        &[&session_ref, &membership.participant_ref(), &enrolled_at],
        "SELECT enrolled_at_unix_ms FROM session_membership \
         WHERE session_ref = $1 AND participant_ref = $2",
        &[&session_ref, &membership.participant_ref()],
        &|row| {
            let stored_enrolled: i64 = row.get(0);
            stored_enrolled == enrolled_at
        },
    )
}

fn persist_consent_record(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    record: &SessionConsentRecord,
    encryption_key: &SessionEncryptionKey,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let sealed = record.sealed_payload(encryption_key, session_ref);
    persist_sealed_row(
        transaction,
        "INSERT INTO session_consent_record (\
             session_ref, event_ref, participant_ref, encryption_nonce, ciphertext_payload\
         ) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (session_ref, event_ref) DO NOTHING",
        "SELECT participant_ref, encryption_nonce, ciphertext_payload \
         FROM session_consent_record WHERE session_ref = $1 AND event_ref = $2",
        session_ref,
        record.event_ref(),
        record.participant_ref(),
        &sealed,
    )
}

fn persist_audit_event(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &SessionAuditEvent,
    encryption_key: &SessionEncryptionKey,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let sealed = event.sealed_payload(encryption_key, session_ref);
    persist_audit_insert(transaction, session_ref, event, &sealed)
}

fn persist_sealed_row(
    transaction: &mut Transaction<'_>,
    insert_sql: &str,
    select_sql: &str,
    session_ref: &str,
    event_ref: &str,
    identity_ref: &str,
    sealed: &SealedPayload,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let nonce = sealed.nonce().as_slice();
    let ciphertext = sealed.ciphertext();
    insert_then_classify(
        transaction,
        insert_sql,
        &[&session_ref, &event_ref, &identity_ref, &nonce, &ciphertext],
        select_sql,
        &[&session_ref, &event_ref],
        &|row| {
            let stored_identity: String = row.get(0);
            let stored_nonce: Vec<u8> = row.get(1);
            let stored_ciphertext: Vec<u8> = row.get(2);
            stored_identity == identity_ref
                && stored_nonce == nonce
                && stored_ciphertext == ciphertext
        },
    )
}

fn persist_export_pointer(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    pointer: Option<&ExportSnapshotPointer>,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let Some(pointer) = pointer else {
        let count: i64 = query_one(
            transaction,
            "SELECT COUNT(*) FROM export_snapshot_pointer WHERE session_ref = $1",
            &[&session_ref],
        )?
        .get(0);
        return if count == 0 {
            Ok(false)
        } else {
            Err(MeasurementSessionPersistenceError::ConflictingReplay)
        };
    };
    let created_at = unix_ms(pointer.created_at_unix_ms())?;
    insert_then_classify(
        transaction,
        "INSERT INTO export_snapshot_pointer (\
             session_ref, snapshot_ref, request_ref, content_digest, created_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (session_ref) DO NOTHING",
        &[
            &session_ref,
            &pointer.snapshot_ref(),
            &pointer.request_ref(),
            &pointer.content_digest(),
            &created_at,
        ],
        "SELECT snapshot_ref, request_ref, content_digest, created_at_unix_ms \
         FROM export_snapshot_pointer WHERE session_ref = $1",
        &[&session_ref],
        &|row| {
            let stored_snapshot: String = row.get(0);
            let stored_request: String = row.get(1);
            let stored_digest: String = row.get(2);
            let stored_created: i64 = row.get(3);
            stored_snapshot == pointer.snapshot_ref()
                && stored_request == pointer.request_ref()
                && stored_digest == pointer.content_digest()
                && stored_created == created_at
        },
    )
}

fn load_session_header(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<StoredSessionHeader>, MeasurementSessionPersistenceError> {
    let rows = query(
        transaction,
        "SELECT tenant_ref, owner_participant_ref, created_at_unix_ms \
         FROM measurement_session WHERE session_ref = $1",
        &[&session_ref],
    )?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(StoredSessionHeader {
        tenant_ref: row.get(0),
        owner_participant_ref: row.get(1),
        created_at_unix_ms: loaded_unix_ms(row.get(2))?,
    }))
}

fn load_memberships(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Vec<SessionMembership>, MeasurementSessionPersistenceError> {
    let rows = query(
        transaction,
        "SELECT m.participant_ref, p.tenant_ref, p.created_at_unix_ms, m.enrolled_at_unix_ms \
         FROM session_membership m \
         INNER JOIN assessment_participant p ON p.participant_ref = m.participant_ref \
         WHERE m.session_ref = $1 \
         ORDER BY m.participant_ref",
        &[&session_ref],
    )?;
    let mut memberships = Vec::new();
    for row in rows {
        memberships.push(SessionMembership::new(
            row.get::<_, String>(0).as_str(),
            row.get::<_, String>(1).as_str(),
            loaded_unix_ms(row.get(2))?,
            loaded_unix_ms(row.get(3))?,
        )?);
    }
    Ok(memberships)
}

fn load_consent_records(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    encryption_key: &SessionEncryptionKey,
) -> Result<Vec<SessionConsentRecord>, MeasurementSessionPersistenceError> {
    let rows = query(
        transaction,
        "SELECT event_ref, participant_ref, encryption_nonce, ciphertext_payload \
         FROM session_consent_record WHERE session_ref = $1 ORDER BY event_ref",
        &[&session_ref],
    )?;
    let mut records = Vec::new();
    for row in rows {
        let event_ref: String = row.get(0);
        let participant_ref: String = row.get(1);
        let nonce: Vec<u8> = row.get(2);
        let ciphertext: Vec<u8> = row.get(3);
        let sealed = SealedPayload::from_stored(&nonce, ciphertext)?;
        records.push(SessionConsentRecord::from_sealed(
            &event_ref,
            &participant_ref,
            encryption_key,
            session_ref,
            &sealed,
        )?);
    }
    Ok(records)
}

fn load_audit_events(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    encryption_key: &SessionEncryptionKey,
) -> Result<Vec<SessionAuditEvent>, MeasurementSessionPersistenceError> {
    let rows = query(
        transaction,
        "SELECT event_ref, actor_ref, occurred_at_unix_ms, encryption_nonce, ciphertext_payload \
         FROM session_audit_event WHERE session_ref = $1 ORDER BY event_ref",
        &[&session_ref],
    )?;
    let mut events = Vec::new();
    for row in rows {
        let event_ref: String = row.get(0);
        let actor_ref: String = row.get(1);
        let occurred_at = loaded_unix_ms(row.get(2))?;
        let nonce: Vec<u8> = row.get(3);
        let ciphertext: Vec<u8> = row.get(4);
        let sealed = SealedPayload::from_stored(&nonce, ciphertext)?;
        events.push(SessionAuditEvent::from_sealed(
            &event_ref,
            &actor_ref,
            occurred_at,
            encryption_key,
            session_ref,
            &sealed,
        )?);
    }
    Ok(events)
}

fn load_export_pointer(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
) -> Result<Option<ExportSnapshotPointer>, MeasurementSessionPersistenceError> {
    let rows = query(
        transaction,
        "SELECT snapshot_ref, request_ref, content_digest, created_at_unix_ms \
         FROM export_snapshot_pointer WHERE session_ref = $1",
        &[&session_ref],
    )?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(ExportSnapshotPointer::new(
        row.get::<_, String>(0).as_str(),
        row.get::<_, String>(1).as_str(),
        row.get::<_, String>(2).as_str(),
        loaded_unix_ms(row.get(3))?,
    )?))
}

fn persist_audit_insert(
    transaction: &mut Transaction<'_>,
    session_ref: &str,
    event: &SessionAuditEvent,
    sealed: &SealedPayload,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let occurred_at = unix_ms(event.occurred_at_unix_ms())?;
    let nonce = sealed.nonce().as_slice();
    let ciphertext = sealed.ciphertext();
    insert_then_classify(
        transaction,
        "INSERT INTO session_audit_event (\
             session_ref, event_ref, actor_ref, occurred_at_unix_ms, encryption_nonce, ciphertext_payload\
         ) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (session_ref, event_ref) DO NOTHING",
        &[
            &session_ref,
            &event.event_ref(),
            &event.actor_ref(),
            &occurred_at,
            &nonce,
            &ciphertext,
        ],
        "SELECT actor_ref, encryption_nonce, ciphertext_payload, occurred_at_unix_ms \
         FROM session_audit_event WHERE session_ref = $1 AND event_ref = $2",
        &[&session_ref, &event.event_ref()],
        &|row| {
            let stored_actor: String = row.get(0);
            let stored_nonce: Vec<u8> = row.get(1);
            let stored_ciphertext: Vec<u8> = row.get(2);
            let stored_occurred: i64 = row.get(3);
            stored_actor == event.actor_ref()
                && stored_nonce == nonce
                && stored_ciphertext == ciphertext
                && stored_occurred == occurred_at
        },
    )
}

fn unix_ms(value: u64) -> Result<i64, MeasurementSessionPersistenceError> {
    i64::try_from(value).map_err(|_| MeasurementSessionPersistenceError::ValueOutOfRange)
}

fn loaded_unix_ms(value: i64) -> Result<u64, MeasurementSessionPersistenceError> {
    u64::try_from(value).map_err(|_| MeasurementSessionPersistenceError::ValueOutOfRange)
}

fn required_reference(reference: &str) -> Result<&str, MeasurementSessionPersistenceError> {
    normalized_reference(reference).ok_or(MeasurementSessionPersistenceError::InvalidReference)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), MeasurementSessionPersistenceError> {
    let row = query_one(transaction, "SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(MeasurementSessionPersistenceError::UnsupportedIsolationLevel)
    }
}

fn insert_then_classify(
    transaction: &mut Transaction<'_>,
    insert_sql: &str,
    insert_params: &[&(dyn ToSql + Sync)],
    select_sql: &str,
    select_params: &[&(dyn ToSql + Sync)],
    same: &dyn Fn(&Row) -> bool,
) -> Result<bool, MeasurementSessionPersistenceError> {
    let inserted = execute(transaction, insert_sql, insert_params)?;
    if inserted == 1 {
        return Ok(true);
    }
    let row = query_one(transaction, select_sql, select_params)?;
    if same(&row) {
        Ok(false)
    } else {
        Err(MeasurementSessionPersistenceError::ConflictingReplay)
    }
}

fn execute(
    transaction: &mut Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<u64, MeasurementSessionPersistenceError> {
    Ok(transaction.execute(sql, params)?)
}

fn query(
    transaction: &mut Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Vec<Row>, MeasurementSessionPersistenceError> {
    Ok(transaction.query(sql, params)?)
}

fn query_one(
    transaction: &mut Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Row, MeasurementSessionPersistenceError> {
    Ok(transaction.query_one(sql, params)?)
}

#[cfg(test)]
mod tests {
    use super::{
        insert_then_classify, loaded_unix_ms, required_reference, unix_ms,
        MeasurementSessionPersistenceError,
    };
    use crate::authorization::AuthorizationError;
    use crate::measurement_session::MeasurementSessionError;
    use std::error::Error;

    #[test]
    fn insert_then_classify_maps_a_conflict_select_database_error() {
        let connection = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
        let mut client = postgres::Client::connect(&connection, postgres::NoTls)
            .expect("isolated CI PostgreSQL database must be reachable");
        client
            .batch_execute(
                "CREATE SCHEMA IF NOT EXISTS measurement_session_persist_test;\
                 SET search_path TO measurement_session_persist_test;\
                 CREATE TABLE IF NOT EXISTS classify_probe (\
                     probe_ref TEXT PRIMARY KEY,\
                     value_text TEXT NOT NULL\
                 );\
                 TRUNCATE classify_probe;\
                 INSERT INTO classify_probe (probe_ref, value_text) VALUES ('probe_alpha', 'one');",
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        let error = insert_then_classify(
            &mut transaction,
            "INSERT INTO classify_probe (probe_ref, value_text) VALUES ($1, $2) \
             ON CONFLICT (probe_ref) DO NOTHING",
            &[&"probe_alpha", &"one"],
            "SELECT value_text FROM classify_probe_missing WHERE probe_ref = $1",
            &[&"probe_alpha"],
            &|_| true,
        )
        .unwrap_err();
        transaction.rollback().unwrap();
        assert_eq!(persistence_error_name(&error), "database");
    }

    fn persistence_error_name(error: &MeasurementSessionPersistenceError) -> &'static str {
        match error {
            MeasurementSessionPersistenceError::InvalidReference => "invalid_reference",
            MeasurementSessionPersistenceError::ConflictingReplay => "conflicting_replay",
            MeasurementSessionPersistenceError::ValueOutOfRange => "value_out_of_range",
            MeasurementSessionPersistenceError::UnsupportedIsolationLevel => {
                "unsupported_isolation_level"
            }
            MeasurementSessionPersistenceError::Unauthorized(_) => "unauthorized",
            MeasurementSessionPersistenceError::Domain(_) => "domain",
            MeasurementSessionPersistenceError::Database(_) => "database",
        }
    }

    #[test]
    fn helpers_and_error_contracts_are_exhaustive() {
        assert_eq!(
            required_reference("session_alpha").unwrap(),
            "session_alpha"
        );
        assert_eq!(
            persistence_error_name(&required_reference(" ").unwrap_err()),
            "invalid_reference"
        );
        assert_eq!(
            persistence_error_name(&required_reference("12").unwrap_err()),
            "invalid_reference"
        );
        assert_eq!(unix_ms(9).unwrap(), 9);
        assert_eq!(
            persistence_error_name(&unix_ms(u64::MAX).unwrap_err()),
            "value_out_of_range"
        );
        assert_eq!(loaded_unix_ms(9).unwrap(), 9);
        assert_eq!(
            persistence_error_name(&loaded_unix_ms(-1).unwrap_err()),
            "value_out_of_range"
        );
        let unauthorized =
            MeasurementSessionPersistenceError::from(AuthorizationError::CrossTenantDenied);
        let domain =
            MeasurementSessionPersistenceError::from(MeasurementSessionError::SealingFailed);
        let invalid =
            MeasurementSessionPersistenceError::from(MeasurementSessionError::InvalidReference);
        let timestamp =
            MeasurementSessionPersistenceError::from(MeasurementSessionError::InvalidTimestamp);
        assert_eq!(persistence_error_name(&unauthorized), "unauthorized");
        assert_eq!(persistence_error_name(&domain), "domain");
        assert_eq!(persistence_error_name(&invalid), "invalid_reference");
        assert_eq!(persistence_error_name(&timestamp), "value_out_of_range");
        for error in [
            MeasurementSessionPersistenceError::InvalidReference,
            MeasurementSessionPersistenceError::ConflictingReplay,
            MeasurementSessionPersistenceError::ValueOutOfRange,
            MeasurementSessionPersistenceError::UnsupportedIsolationLevel,
            unauthorized,
            MeasurementSessionPersistenceError::from(MeasurementSessionError::SealingFailed),
        ] {
            assert!(!error.to_string().is_empty());
            assert!(!persistence_error_name(&error).is_empty());
            let _ = error.source();
        }
        assert!(MeasurementSessionPersistenceError::ConflictingReplay
            .source()
            .is_none());
    }
}
