//! `PostgreSQL` 18 persistence adapter for integration evidence.
//!
//! The adapter receives a caller-owned `PostgreSQL` client or transaction. It never
//! owns credentials or a cross-service connection. The migration and write paths
//! preserve the immutable integration-event, outbox, and inbox contracts defined
//! by [`crate::integration`].

use crate::integration::{DeliveryOutcome, InboxDisposition, IntegrationEvent, OutboxState};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const INTEGRATION_MIGRATION: &str = include_str!("../migrations/0001_integration_delivery.sql");
const OUTBOX_LEASE_MIGRATION: &str = include_str!("../migrations/0013_outbox_delivery_lease.sql");
const OUTBOX_REPLAY_QUERY: &str = "SELECT EXISTS (\
     SELECT 1 FROM integration_outbox \
     WHERE event_ref = $1 AND event_type = $2 AND schema_version = $3 \
       AND source_ref = $4 AND tenant_ref = $5 AND subject_ref = $6 \
       AND occurred_at_unix_ms = $7 AND correlation_ref = $8 \
       AND causation_ref IS NOT DISTINCT FROM $9 \
       AND payload_digest = $10 AND max_attempts = $11\
 )";
const INBOX_REPLAY_QUERY: &str = "SELECT EXISTS (\
     SELECT 1 FROM integration_inbox \
     WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
       AND source_event_ref = $4 AND event_type = $5 AND schema_version = $6 \
       AND subject_ref = $7 AND payload_digest = $8\
 )";

/// Outcome of inserting immutable persistence evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PersistenceDisposition {
    /// The immutable evidence was inserted for the first time.
    Inserted,
    /// Identical immutable evidence already existed.
    Duplicate,
}

/// Durable result of recording one outbox delivery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryAttemptPersistence {
    disposition: PersistenceDisposition,
    outbox_state: OutboxState,
}

impl DeliveryAttemptPersistence {
    /// Return whether this call inserted new evidence or replayed exact evidence.
    #[must_use]
    pub const fn disposition(self) -> PersistenceDisposition {
        self.disposition
    }

    /// Return the durable outbox state after applying or replaying the attempt.
    #[must_use]
    pub const fn outbox_state(self) -> OutboxState {
        self.outbox_state
    }
}

/// Composite durable identity for one persisted outbox event.
///
/// The three references form the database key used to lock, replay, and advance one
/// outbox entry. Validation remains fail-closed in [`record_outbox_delivery_attempt`]
/// so constructing this lightweight value never bypasses persistence checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboxPersistenceIdentity<'a> {
    source: &'a str,
    tenant: &'a str,
    event: &'a str,
}

impl<'a> OutboxPersistenceIdentity<'a> {
    /// Group the source, tenant, and event references that identify one durable outbox row.
    #[must_use]
    pub const fn new(source_ref: &'a str, tenant_ref: &'a str, event_ref: &'a str) -> Self {
        Self {
            source: source_ref,
            tenant: tenant_ref,
            event: event_ref,
        }
    }
}

/// Durable exclusive-delivery evidence returned by an atomic outbox claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedOutboxLease {
    worker_ref: String,
    lease_ref: String,
    fencing_token: u64,
    expires_at_unix_ms: u64,
}

impl PersistedOutboxLease {
    /// Return the opaque worker identity that owns this persisted delivery lease.
    #[must_use]
    pub fn worker_ref(&self) -> &str {
        &self.worker_ref
    }

    /// Return the opaque lease identity for this persisted delivery attempt.
    #[must_use]
    pub fn lease_ref(&self) -> &str {
        &self.lease_ref
    }

    /// Return the monotonically increasing stale-worker fencing token.
    #[must_use]
    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    /// Return the caller-supplied lease expiry instant persisted with the claim.
    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

/// Fail-closed persistence error for integration evidence.
#[derive(Debug)]
#[non_exhaustive]
pub enum PersistenceError {
    /// An adapter input reference was blank or numeric-only.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// The configured outbox delivery-attempt limit was zero.
    InvalidAttemptLimit,
    /// A runtime value cannot be represented by the bounded `PostgreSQL` column.
    ValueOutOfRange,
    /// The caller's transaction isolation cannot preserve this adapter's replay semantics.
    UnsupportedIsolationLevel,
    /// An idempotency identity already exists with different immutable evidence.
    ConflictingReplay,
    /// A delivery attempt references an outbox entry that does not exist.
    OutboxNotFound,
    /// A delivery attempt timestamp precedes the latest accepted outbox evidence.
    NonMonotonicTimestamp,
    /// A new delivery attempt was requested after the outbox became terminal.
    TerminalOutboxState,
    /// Stored outbox state does not match the migration-defined state vocabulary.
    InvalidStoredState,
    /// Lease expiry was not strictly later than the claim instant.
    InvalidLeaseWindow,
    /// A fencing token was zero and therefore cannot represent a persisted attempt.
    InvalidFencingToken,
    /// An unfenced delivery attempt was submitted against a live exclusive lease.
    OutboxLeaseHeld,
    /// The requested outbox is not currently available for a new exclusive claim.
    NotLeaseable,
    /// A fenced transition was submitted for an outbox without a current lease.
    NotLeased,
    /// Expiry recovery was requested while the persisted lease is still live.
    LeaseStillActive,
    /// A worker presented a fencing token that does not own the current lease.
    StaleLease,
    /// The database clock reached or passed the persisted lease expiry.
    LeaseExpired,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for PersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "persistence references must be opaque non-numeric values",
            Self::InvalidTimestamp => "persistence timestamps must be greater than zero",
            Self::InvalidAttemptLimit => "outbox maximum attempts must be greater than zero",
            Self::ValueOutOfRange => "persistence value exceeds the supported PostgreSQL range",
            Self::UnsupportedIsolationLevel => {
                "PostgreSQL integration persistence requires read committed isolation"
            }
            Self::ConflictingReplay => {
                "persistence idempotency identity was replayed with conflicting evidence"
            }
            Self::OutboxNotFound => "delivery attempt references an unknown outbox entry",
            Self::NonMonotonicTimestamp => {
                "delivery attempt timestamp precedes the latest outbox evidence"
            }
            Self::TerminalOutboxState => "terminal outbox state rejects new delivery attempts",
            Self::InvalidStoredState => "stored outbox state violates the persistence contract",
            Self::InvalidLeaseWindow => "outbox lease expiry must be later than claim time",
            Self::InvalidFencingToken => "outbox lease fencing tokens must be positive",
            Self::OutboxLeaseHeld => {
                "live outbox delivery lease rejects unfenced delivery attempts"
            }
            Self::NotLeaseable => {
                "outbox is not currently available for an exclusive delivery lease"
            }
            Self::NotLeased => "outbox does not currently have a delivery lease",
            Self::LeaseStillActive => "outbox delivery lease has not expired",
            Self::StaleLease => "outbox delivery fencing token is stale",
            Self::LeaseExpired => "outbox delivery lease has expired",
            Self::Database(_) => "PostgreSQL persistence operation failed",
        })
    }
}

impl Error for PersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for PersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent integration-evidence migration to a `PostgreSQL` connection.
///
/// The caller owns the connection, transaction policy, credentials, TLS policy, and
/// deployment routing. The repository-owned table and lease phases are submitted in
/// one `PostgreSQL` simple-query batch so a plain [`postgres::Client`] cannot commit
/// the first phase when the second phase fails. When the caller supplies a
/// [`postgres::Transaction`], the same batch remains inside that caller-owned transaction.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if any migration phase cannot be applied.
pub fn apply_integration_migration(client: &mut impl GenericClient) -> Result<(), postgres::Error> {
    let mut migration =
        String::with_capacity(INTEGRATION_MIGRATION.len() + OUTBOX_LEASE_MIGRATION.len() + 1);
    migration.push_str(INTEGRATION_MIGRATION);
    migration.push('\n');
    migration.push_str(OUTBOX_LEASE_MIGRATION);
    client.batch_execute(&migration)
}

/// Insert an immutable integration event into the durable outbox.
///
/// The durable identity is `(source_ref, tenant_ref, event_ref)`, because an event
/// reference is not globally unique across bounded-context sources or tenants. A
/// repeated scoped identity is idempotent only when every immutable event field and
/// `max_attempts` match the existing row. Reuse with different evidence fails closed.
/// The function uses the caller-owned client/transaction, so a domain mutation and
/// this outbox insert can be committed atomically by the caller.
///
/// The current insert-then-inspect replay algorithm requires `PostgreSQL` `READ
/// COMMITTED` isolation. That isolation refreshes the statement snapshot after an
/// `ON CONFLICT DO NOTHING` wait so the exact conflicting row can be inspected.
/// Stronger transaction isolation is rejected rather than misclassifying a replay.
///
/// # Errors
///
/// Returns [`PersistenceError`] for an invalid attempt limit, a value outside the
/// supported `PostgreSQL` integer range, unsupported transaction isolation,
/// conflicting replay, or a database failure.
pub fn enqueue_outbox_event(
    client: &mut impl GenericClient,
    event: &IntegrationEvent,
    max_attempts: usize,
) -> Result<PersistenceDisposition, PersistenceError> {
    if max_attempts == 0 {
        return Err(PersistenceError::InvalidAttemptLimit);
    }
    let occurred_at_unix_ms = postgres_bigint(event.occurred_at_unix_ms())?;
    let max_attempts =
        i32::try_from(max_attempts).map_err(|_| PersistenceError::ValueOutOfRange)?;
    require_read_committed(client)?;
    let causation_ref = event.causation_ref();
    let params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.source(),
        &event.tenant_ref(),
        &event.subject_ref(),
        &occurred_at_unix_ms,
        &event.correlation_ref(),
        &causation_ref,
        &event.payload_digest(),
        &max_attempts,
    ];

    let inserted = client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
             occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest,\
             max_attempts, current_state, latest_event_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending', $7) \
         ON CONFLICT (source_ref, tenant_ref, event_ref) DO NOTHING",
        params,
    )?;
    if inserted == 1 {
        return Ok(PersistenceDisposition::Inserted);
    }

    let exact_replay_row = client.query_one(OUTBOX_REPLAY_QUERY, params)?;
    let exact_replay: bool = exact_replay_row.get(0);
    if exact_replay {
        Ok(PersistenceDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

/// Persist one immutable outbox delivery attempt and atomically advance delivery state.
///
/// The caller must supply the transaction so insertion of immutable attempt evidence
/// and the corresponding outbox-state transition cannot commit independently. The
/// outbox row is locked before replay classification or mutation, serializing adapter
/// calls for one [`OutboxPersistenceIdentity`]. Exact attempt replay remains
/// idempotent even after terminal delivery or quarantine. A different replay of the
/// same `attempt_ref`, backward time, unknown outbox, or new attempt after a terminal
/// state fails closed.
///
/// Retryable failure keeps the outbox pending while automatic-attempt budget remains;
/// exhausting that budget quarantines the outbox. Delivered attempts terminally mark
/// the outbox delivered, while permanent failures quarantine immediately.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid references/timestamps, unsupported
/// transaction isolation, unknown or terminal outbox state, backward event time,
/// conflicting replay evidence, invalid stored state, or a database failure.
pub fn record_outbox_delivery_attempt(
    transaction: &mut Transaction<'_>,
    identity: OutboxPersistenceIdentity<'_>,
    attempt_ref: &str,
    outcome: DeliveryOutcome,
    occurred_at_unix_ms: u64,
    cause_code: Option<&str>,
) -> Result<DeliveryAttemptPersistence, PersistenceError> {
    let source_ref = required_persistence_reference(identity.source)?;
    let tenant_ref = required_persistence_reference(identity.tenant)?;
    let event_ref = required_persistence_reference(identity.event)?;
    let attempt_ref = required_persistence_reference(attempt_ref)?;
    if occurred_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let cause_code = cause_code.map(required_persistence_reference).transpose()?;
    let occurred_at_unix_ms = postgres_bigint(occurred_at_unix_ms)?;
    require_read_committed(transaction)?;

    let outbox_row = transaction
        .query_opt(
            "SELECT max_attempts, current_state, latest_event_at_unix_ms, lease_worker_ref
             FROM integration_outbox
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3
             FOR UPDATE",
            &[&source_ref, &tenant_ref, &event_ref],
        )?
        .ok_or(PersistenceError::OutboxNotFound)?;
    let max_attempts: i32 = outbox_row.get(0);
    let stored_state: String = outbox_row.get(1);
    let current_state = parse_outbox_state(&stored_state)?;
    let latest_event_at_unix_ms: i64 = outbox_row.get(2);
    let lease_worker_ref: Option<String> = outbox_row.get(3);
    if lease_worker_ref.is_some() {
        return Err(PersistenceError::OutboxLeaseHeld);
    }

    let write = DeliveryAttemptWrite {
        source_ref,
        tenant_ref,
        event_ref,
        attempt_ref,
        outcome,
        occurred_at_unix_ms,
        cause_code,
        current_state,
        latest_event_at_unix_ms,
        max_attempts,
        clear_lease: false,
    };
    if let Some(replay) = classify_existing_delivery_attempt(transaction, &write)? {
        return Ok(replay);
    }
    persist_new_delivery_attempt(transaction, &write)
}

/// Claim exclusive delivery ownership of one pending outbox row.
///
/// The claim succeeds only when the row is `pending` and has no current lease,
/// including an expired unrecovered lease. A later [`expire_outbox_delivery_lease`]
/// clears the expired fence so the next claim issues a new token. `READ COMMITTED`
/// is required so concurrent claims observe the latest committed lease evidence.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid identity, an empty lease window,
/// unsupported isolation, a missing or non-claimable outbox, or a database failure.
pub fn claim_outbox_delivery(
    transaction: &mut Transaction<'_>,
    identity: OutboxPersistenceIdentity<'_>,
    worker_ref: &str,
    lease_ref: &str,
    claimed_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<PersistedOutboxLease, PersistenceError> {
    let source_ref = required_persistence_reference(identity.source)?;
    let tenant_ref = required_persistence_reference(identity.tenant)?;
    let event_ref = required_persistence_reference(identity.event)?;
    let worker_ref = required_persistence_reference(worker_ref)?;
    let lease_ref = required_persistence_reference(lease_ref)?;
    if claimed_at_unix_ms == 0 || expires_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let claimed_at_unix_ms = postgres_bigint(claimed_at_unix_ms)?;
    let expires_at_unix_ms = postgres_bigint(expires_at_unix_ms)?;
    if expires_at_unix_ms <= claimed_at_unix_ms {
        return Err(PersistenceError::InvalidLeaseWindow);
    }
    require_read_committed(transaction)?;

    let claimed = transaction.query_opt(
        "UPDATE integration_outbox
         SET lease_worker_ref = $4,
             lease_ref = $5,
             lease_fencing_token = delivery_lease_generation + 1,
             lease_expires_at_unix_ms = $6,
             delivery_lease_generation = delivery_lease_generation + 1
         WHERE source_ref = $1
           AND tenant_ref = $2
           AND event_ref = $3
           AND current_state = 'pending'
           AND lease_worker_ref IS NULL
         RETURNING lease_fencing_token",
        &[
            &source_ref,
            &tenant_ref,
            &event_ref,
            &worker_ref,
            &lease_ref,
            &expires_at_unix_ms,
        ],
    )?;
    if let Some(row) = claimed {
        let fencing_token: i64 = row.get(0);
        return Ok(PersistedOutboxLease {
            worker_ref: worker_ref.to_owned(),
            lease_ref: lease_ref.to_owned(),
            fencing_token: postgres_u64(fencing_token)?,
            expires_at_unix_ms: postgres_u64(expires_at_unix_ms)?,
        });
    }

    let exists = match transaction.query_opt(
        "SELECT 1 FROM integration_outbox
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
        &[&source_ref, &tenant_ref, &event_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(PersistenceError::from(error)),
    };
    if exists.is_some() {
        Err(PersistenceError::NotLeaseable)
    } else {
        Err(PersistenceError::OutboxNotFound)
    }
}

/// Recover one expired exclusive outbox delivery lease without transferring the fence.
///
/// The row stays `pending` and the lease columns are cleared. A later
/// [`claim_outbox_delivery`] issues the next fencing token. `READ COMMITTED` is
/// required so concurrent expiry recovery observes the latest committed lease.
///
/// `observed_at_unix_ms` must be a positive caller observation, but liveness is
/// classified from the database clock. A future caller timestamp cannot steal a
/// lease that is still live on `clock_timestamp()`.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid identity or timestamp, unsupported
/// isolation, a missing or unleased outbox, a still-live lease, or a database
/// failure.
pub fn expire_outbox_delivery_lease(
    transaction: &mut Transaction<'_>,
    identity: OutboxPersistenceIdentity<'_>,
    observed_at_unix_ms: u64,
) -> Result<(), PersistenceError> {
    let source_ref = required_persistence_reference(identity.source)?;
    let tenant_ref = required_persistence_reference(identity.tenant)?;
    let event_ref = required_persistence_reference(identity.event)?;
    if observed_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let _observed_at_unix_ms = postgres_bigint(observed_at_unix_ms)?;
    require_read_committed(transaction)?;

    let recovered = match transaction.query_opt(
        "UPDATE integration_outbox
         SET lease_worker_ref = NULL,
             lease_ref = NULL,
             lease_fencing_token = NULL,
             lease_expires_at_unix_ms = NULL
         WHERE source_ref = $1
           AND tenant_ref = $2
           AND event_ref = $3
           AND lease_worker_ref IS NOT NULL
           AND lease_expires_at_unix_ms
               <= floor(extract(epoch FROM clock_timestamp()) * 1000)::bigint
         RETURNING event_ref",
        &[&source_ref, &tenant_ref, &event_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(PersistenceError::from(error)),
    };
    if recovered.is_some() {
        return Ok(());
    }

    let row = match transaction.query_opt(
        "SELECT lease_worker_ref IS NOT NULL AS leased
         FROM integration_outbox
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
        &[&source_ref, &tenant_ref, &event_ref],
    ) {
        Ok(row) => row,
        Err(error) => return Err(PersistenceError::from(error)),
    };
    let Some(row) = row else {
        return Err(PersistenceError::OutboxNotFound);
    };
    let leased: bool = row.get(0);
    if leased {
        Err(PersistenceError::LeaseStillActive)
    } else {
        Err(PersistenceError::NotLeased)
    }
}

/// Persist a delivery attempt for the worker that owns the current fenced lease.
///
/// Exact attempt replay remains idempotent after a completed attempt has cleared its
/// lease. While a lease is present, the database clock and fencing token are checked
/// before replay classification so stale or expired workers fail closed.
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid identity, a zero fencing token,
/// unsupported isolation, a missing or unleased outbox, a stale or expired fence,
/// conflicting replay, or a database failure.
pub fn record_leased_outbox_delivery_attempt(
    transaction: &mut Transaction<'_>,
    identity: OutboxPersistenceIdentity<'_>,
    attempt_ref: &str,
    outcome: DeliveryOutcome,
    occurred_at_unix_ms: u64,
    cause_code: Option<&str>,
    fencing_token: u64,
) -> Result<DeliveryAttemptPersistence, PersistenceError> {
    let source_ref = required_persistence_reference(identity.source)?;
    let tenant_ref = required_persistence_reference(identity.tenant)?;
    let event_ref = required_persistence_reference(identity.event)?;
    let attempt_ref = required_persistence_reference(attempt_ref)?;
    if occurred_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    if fencing_token == 0 {
        return Err(PersistenceError::InvalidFencingToken);
    }
    let cause_code = cause_code.map(required_persistence_reference).transpose()?;
    let occurred_at_unix_ms = postgres_bigint(occurred_at_unix_ms)?;
    let fencing_token = postgres_bigint(fencing_token)?;
    require_read_committed(transaction)?;

    let outbox_row = match transaction.query_opt(
        "SELECT max_attempts, current_state, latest_event_at_unix_ms,
                lease_fencing_token, lease_expires_at_unix_ms
         FROM integration_outbox
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3
         FOR UPDATE",
        &[&source_ref, &tenant_ref, &event_ref],
    ) {
        Ok(Some(row)) => row,
        Ok(None) => return Err(PersistenceError::OutboxNotFound),
        Err(error) => return Err(PersistenceError::from(error)),
    };
    let max_attempts: i32 = outbox_row.get(0);
    let stored_state: String = outbox_row.get(1);
    let current_state = parse_outbox_state(&stored_state)?;
    let latest_event_at_unix_ms: i64 = outbox_row.get(2);
    let stored_fence: Option<i64> = outbox_row.get(3);
    let lease_expires_at_unix_ms: Option<i64> = outbox_row.get(4);
    let database_now_unix_ms: i64 = transaction
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp()) * 1000)::bigint",
            &[],
        )?
        .get(0);

    let write = DeliveryAttemptWrite {
        source_ref,
        tenant_ref,
        event_ref,
        attempt_ref,
        outcome,
        occurred_at_unix_ms,
        cause_code,
        current_state,
        latest_event_at_unix_ms,
        max_attempts,
        clear_lease: true,
    };

    match (stored_fence, lease_expires_at_unix_ms) {
        (Some(stored_fence), Some(lease_expires_at_unix_ms)) => {
            if stored_fence != fencing_token {
                return Err(PersistenceError::StaleLease);
            }
            if lease_expires_at_unix_ms <= database_now_unix_ms {
                return Err(PersistenceError::LeaseExpired);
            }
            if let Some(replay) = classify_existing_delivery_attempt(transaction, &write)? {
                return Ok(replay);
            }
        }
        (None, None) => {
            if let Some(replay) = classify_existing_delivery_attempt(transaction, &write)? {
                return Ok(replay);
            }
            return Err(PersistenceError::NotLeased);
        }
        _ => return Err(PersistenceError::NotLeased),
    }

    persist_new_delivery_attempt(transaction, &write)
}

/// Accept or deduplicate one immutable tenant-bound event in a durable inbox.
///
/// The durable deduplication identity is
/// `(consumer_ref, source_ref, tenant_ref, source_event_ref)`. The first accepted
/// receive timestamp is retained. A later exact replay is reported as duplicate;
/// a replay with different event type, schema version, subject, or payload digest
/// fails closed.
///
/// The current insert-then-inspect replay algorithm requires `PostgreSQL` `READ
/// COMMITTED` isolation for the same statement-snapshot reason as
/// [`enqueue_outbox_event`].
///
/// # Errors
///
/// Returns [`PersistenceError`] for invalid consumer identity, invalid/out-of-range
/// receive time, unsupported transaction isolation, conflicting replay evidence,
/// or database failure.
pub fn accept_inbox_event(
    client: &mut impl GenericClient,
    consumer_ref: &str,
    event: &IntegrationEvent,
    received_at_unix_ms: u64,
) -> Result<InboxDisposition, PersistenceError> {
    let consumer_ref =
        normalized_reference(consumer_ref).ok_or(PersistenceError::InvalidReference)?;
    if received_at_unix_ms == 0 {
        return Err(PersistenceError::InvalidTimestamp);
    }
    let received_at_unix_ms = postgres_bigint(received_at_unix_ms)?;
    require_read_committed(client)?;

    let replay_params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &consumer_ref,
        &event.source(),
        &event.tenant_ref(),
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.subject_ref(),
        &event.payload_digest(),
    ];
    let insert_params: &[&(dyn postgres::types::ToSql + Sync)] = &[
        &consumer_ref,
        &event.source(),
        &event.tenant_ref(),
        &event.event_ref(),
        &event.event_type(),
        &event.schema_version(),
        &event.subject_ref(),
        &event.payload_digest(),
        &received_at_unix_ms,
    ];

    let inserted = client.execute(
        "INSERT INTO integration_inbox (\
             consumer_ref, source_ref, tenant_ref, source_event_ref, event_type, schema_version,\
             subject_ref, payload_digest, received_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (consumer_ref, source_ref, tenant_ref, source_event_ref) DO NOTHING",
        insert_params,
    )?;
    if inserted == 1 {
        return Ok(InboxDisposition::Accepted);
    }

    let exact_replay_row = client.query_one(INBOX_REPLAY_QUERY, replay_params)?;
    let exact_replay: bool = exact_replay_row.get(0);
    if exact_replay {
        Ok(InboxDisposition::Duplicate)
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

struct DeliveryAttemptWrite<'a> {
    source_ref: &'a str,
    tenant_ref: &'a str,
    event_ref: &'a str,
    attempt_ref: &'a str,
    outcome: DeliveryOutcome,
    occurred_at_unix_ms: i64,
    cause_code: Option<&'a str>,
    current_state: OutboxState,
    latest_event_at_unix_ms: i64,
    max_attempts: i32,
    clear_lease: bool,
}

fn classify_existing_delivery_attempt(
    transaction: &mut Transaction<'_>,
    write: &DeliveryAttemptWrite<'_>,
) -> Result<Option<DeliveryAttemptPersistence>, PersistenceError> {
    let outcome_name = delivery_outcome_name(write.outcome);
    let Some(existing_attempt) = transaction.query_opt(
        "SELECT delivery_outcome, occurred_at_unix_ms, cause_code
         FROM integration_delivery_attempt
         WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3 AND attempt_ref = $4",
        &[
            &write.source_ref,
            &write.tenant_ref,
            &write.event_ref,
            &write.attempt_ref,
        ],
    )?
    else {
        return Ok(None);
    };
    let existing_outcome: String = existing_attempt.get(0);
    let existing_occurred_at_unix_ms: i64 = existing_attempt.get(1);
    let existing_cause_code: Option<String> = existing_attempt.get(2);
    if existing_outcome == outcome_name
        && existing_occurred_at_unix_ms == write.occurred_at_unix_ms
        && existing_cause_code.as_deref() == write.cause_code
    {
        Ok(Some(DeliveryAttemptPersistence {
            disposition: PersistenceDisposition::Duplicate,
            outbox_state: write.current_state,
        }))
    } else {
        Err(PersistenceError::ConflictingReplay)
    }
}

fn persist_new_delivery_attempt(
    transaction: &mut Transaction<'_>,
    write: &DeliveryAttemptWrite<'_>,
) -> Result<DeliveryAttemptPersistence, PersistenceError> {
    if write.current_state != OutboxState::Pending {
        return Err(PersistenceError::TerminalOutboxState);
    }
    if write.occurred_at_unix_ms < write.latest_event_at_unix_ms {
        return Err(PersistenceError::NonMonotonicTimestamp);
    }

    let outcome_name = delivery_outcome_name(write.outcome);
    // PostgreSQL evaluates the main SELECT against the statement snapshot, so rows
    // inserted by this data-modifying CTE are counted only through inserted_attempt.
    let attempt_count: i64 = transaction
        .query_one(
            "WITH inserted_attempt AS (
                 INSERT INTO integration_delivery_attempt (
                     source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome,
                     occurred_at_unix_ms, cause_code
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                 RETURNING 1
             )
             SELECT count(*) + (SELECT count(*) FROM inserted_attempt)
             FROM integration_delivery_attempt
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[
                &write.source_ref,
                &write.tenant_ref,
                &write.event_ref,
                &write.attempt_ref,
                &outcome_name,
                &write.occurred_at_unix_ms,
                &write.cause_code,
            ],
        )?
        .get(0);
    let next_state = match write.outcome {
        DeliveryOutcome::Delivered => OutboxState::Delivered,
        DeliveryOutcome::PermanentFailure => OutboxState::Quarantined,
        DeliveryOutcome::RetryableFailure if attempt_count >= i64::from(write.max_attempts) => {
            OutboxState::Quarantined
        }
        DeliveryOutcome::RetryableFailure => OutboxState::Pending,
    };
    let next_state_name = outbox_state_name(next_state);
    let update_result = if write.clear_lease {
        transaction.execute(
            "UPDATE integration_outbox
             SET current_state = $4,
                 latest_event_at_unix_ms = $5,
                 lease_worker_ref = NULL,
                 lease_ref = NULL,
                 lease_fencing_token = NULL,
                 lease_expires_at_unix_ms = NULL
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[
                &write.source_ref,
                &write.tenant_ref,
                &write.event_ref,
                &next_state_name,
                &write.occurred_at_unix_ms,
            ],
        )
    } else {
        transaction.execute(
            "UPDATE integration_outbox
             SET current_state = $4, latest_event_at_unix_ms = $5
             WHERE source_ref = $1 AND tenant_ref = $2 AND event_ref = $3",
            &[
                &write.source_ref,
                &write.tenant_ref,
                &write.event_ref,
                &next_state_name,
                &write.occurred_at_unix_ms,
            ],
        )
    };
    if let Err(error) = update_result {
        return Err(PersistenceError::from(error));
    }

    Ok(DeliveryAttemptPersistence {
        disposition: PersistenceDisposition::Inserted,
        outbox_state: next_state,
    })
}

fn required_persistence_reference(reference: &str) -> Result<&str, PersistenceError> {
    normalized_reference(reference).ok_or(PersistenceError::InvalidReference)
}

fn delivery_outcome_name(outcome: DeliveryOutcome) -> &'static str {
    match outcome {
        DeliveryOutcome::Delivered => "delivered",
        DeliveryOutcome::RetryableFailure => "retryable_failure",
        DeliveryOutcome::PermanentFailure => "permanent_failure",
    }
}

fn outbox_state_name(state: OutboxState) -> &'static str {
    match state {
        OutboxState::Pending => "pending",
        OutboxState::Delivered => "delivered",
        OutboxState::Quarantined => "quarantined",
    }
}

fn parse_outbox_state(state: &str) -> Result<OutboxState, PersistenceError> {
    match state {
        "pending" => Ok(OutboxState::Pending),
        "delivered" => Ok(OutboxState::Delivered),
        "quarantined" => Ok(OutboxState::Quarantined),
        _ => Err(PersistenceError::InvalidStoredState),
    }
}

fn require_read_committed(client: &mut impl GenericClient) -> Result<(), PersistenceError> {
    let row = client.query_one("SHOW transaction_isolation", &[])?;
    let isolation_level: String = row.get(0);
    if isolation_level == "read committed" {
        Ok(())
    } else {
        Err(PersistenceError::UnsupportedIsolationLevel)
    }
}

fn postgres_bigint(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::ValueOutOfRange)
}

fn postgres_u64(value: i64) -> Result<u64, PersistenceError> {
    match u64::try_from(value) {
        Ok(value) => Ok(value),
        Err(_) => Err(PersistenceError::ValueOutOfRange),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_outbox_state, postgres_u64, PersistenceError};
    use crate::integration::OutboxState;

    #[test]
    fn lease_persistence_errors_are_safe_and_specific() {
        for (error, expected) in [
            (
                PersistenceError::InvalidLeaseWindow,
                "outbox lease expiry must be later than claim time",
            ),
            (
                PersistenceError::InvalidFencingToken,
                "outbox lease fencing tokens must be positive",
            ),
            (
                PersistenceError::OutboxLeaseHeld,
                "live outbox delivery lease rejects unfenced delivery attempts",
            ),
            (
                PersistenceError::NotLeaseable,
                "outbox is not currently available for an exclusive delivery lease",
            ),
            (
                PersistenceError::NotLeased,
                "outbox does not currently have a delivery lease",
            ),
            (
                PersistenceError::LeaseStillActive,
                "outbox delivery lease has not expired",
            ),
            (
                PersistenceError::StaleLease,
                "outbox delivery fencing token is stale",
            ),
            (
                PersistenceError::LeaseExpired,
                "outbox delivery lease has expired",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    #[test]
    fn stored_outbox_state_parser_is_fail_closed() {
        assert_eq!(parse_outbox_state("pending").unwrap(), OutboxState::Pending);
        assert_eq!(
            parse_outbox_state("delivered").unwrap(),
            OutboxState::Delivered
        );
        assert_eq!(
            parse_outbox_state("quarantined").unwrap(),
            OutboxState::Quarantined
        );
        assert!(matches!(
            parse_outbox_state("unexpected"),
            Err(PersistenceError::InvalidStoredState)
        ));
    }

    #[test]
    fn persisted_lease_tokens_reject_negative_database_values() {
        assert!(matches!(
            postgres_u64(-1),
            Err(PersistenceError::ValueOutOfRange)
        ));
        assert_eq!(postgres_u64(1).unwrap(), 1);
    }
}
