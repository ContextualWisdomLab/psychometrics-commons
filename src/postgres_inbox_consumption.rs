//! `PostgreSQL` 18 persistence for inbox side-effect consumption.
//!
//! Receipt in `integration_inbox` is not completion. This adapter stores one
//! durable consumption row per inbox identity and work item, then claims,
//! completes, or quarantines that row under `READ COMMITTED` with fencing.

use crate::integration::{ConsumptionState, InboxConsumption};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Transaction};
use std::error::Error;
use std::fmt::{Display, Formatter};

const INBOX_CONSUMPTION_MIGRATION: &str =
    include_str!("../migrations/0012_integration_consumption.sql");
const INBOX_CLAIM_EXPIRY_GUARD_MIGRATION: &str =
    include_str!("../migrations/0019_inbox_claim_expiry_guard.sql");

/// Outcome of persisting one pending inbox consumption identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InboxConsumptionDisposition {
    /// A new pending consumption row was inserted.
    Inserted,
    /// The same immutable consumption identity already existed.
    Duplicate,
}

/// Fail-closed error for durable inbox-consumption persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum InboxConsumptionPersistenceError {
    /// A consumption, inbox, tenant, or evidence identity was blank or numeric-like.
    InvalidReference,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// A runtime value cannot be represented by the bounded `PostgreSQL` column.
    ValueOutOfRange,
    /// Inbox-consumption persistence requires `PostgreSQL` `READ COMMITTED`.
    UnsupportedIsolationLevel,
    /// Consumption identity was replayed with different immutable evidence.
    ConflictingReplay,
    /// The referenced inbox receipt does not exist.
    InboxNotFound,
    /// The referenced consumption row does not exist.
    ConsumptionNotFound,
    /// A new transition was requested after completion or quarantine.
    TerminalConsumptionState,
    /// A claim was requested for a consumption that is not pending.
    ConsumptionNotClaimable,
    /// A completion or quarantine used a fencing token that is no longer current.
    StaleConsumptionFence,
    /// A transition timestamp precedes the latest accepted consumption evidence.
    NonMonotonicTimestamp,
    /// Stored consumption state does not match the migration-defined vocabulary.
    InvalidStoredState,
    /// A pending consumption was offered with a non-fresh domain shape.
    UnsupportedInitialState,
    /// A processing claim expiry is not later than its claim time.
    InvalidConsumptionClaimWindow,
    /// Claim expiry was requested before the stored processing claim expired.
    ConsumptionClaimStillActive,
    /// Claim expiry was requested for a consumption that is not processing.
    ConsumptionNotProcessing,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for InboxConsumptionPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "inbox consumption persistence references must be opaque values"
            }
            Self::InvalidTimestamp => "inbox consumption timestamps must be greater than zero",
            Self::ValueOutOfRange => {
                "inbox consumption value exceeds the supported PostgreSQL range"
            }
            Self::UnsupportedIsolationLevel => {
                "inbox consumption persistence requires read committed isolation"
            }
            Self::ConflictingReplay => {
                "inbox consumption identity was replayed with conflicting evidence"
            }
            Self::InboxNotFound => "inbox consumption references an unknown inbox receipt",
            Self::ConsumptionNotFound => "inbox consumption row does not exist",
            Self::TerminalConsumptionState => {
                "terminal inbox consumption rejects a new processing transition"
            }
            Self::ConsumptionNotClaimable => {
                "inbox consumption can be claimed only from the pending state"
            }
            Self::StaleConsumptionFence => {
                "inbox consumption fencing token does not match the current claim"
            }
            Self::NonMonotonicTimestamp => {
                "inbox consumption timestamp precedes the latest accepted evidence"
            }
            Self::InvalidStoredState => {
                "stored inbox consumption state violates the persistence contract"
            }
            Self::UnsupportedInitialState => {
                "inbox consumption persist accepts only a fresh pending domain state"
            }
            Self::InvalidConsumptionClaimWindow => {
                "inbox consumption claim expiry must be later than claim time"
            }
            Self::ConsumptionClaimStillActive => {
                "inbox consumption processing claim has not expired"
            }
            Self::ConsumptionNotProcessing => {
                "inbox consumption claim expiry requires the processing state"
            }
            Self::Database(_) => "PostgreSQL inbox-consumption persistence failed",
        })
    }
}

impl Error for InboxConsumptionPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for InboxConsumptionPersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent inbox-consumption migrations to a `PostgreSQL` connection.
///
/// The shipped base migration remains immutable; forward-only hardening is
/// applied afterwards so existing installations receive the same guard as a
/// newly initialized database.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if either migration cannot be applied.
pub fn apply_inbox_consumption_migration(
    client: &mut impl GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(INBOX_CONSUMPTION_MIGRATION)?;
    client.batch_execute(INBOX_CLAIM_EXPIRY_GUARD_MIGRATION)
}

/// Persist one pending consumption identity for an existing inbox receipt.
///
/// Exact replay of the same scoped identity and side-effect reference is
/// idempotent. Rebinding any stored field fails closed. Receipt-only inbox
/// rows remain uncompleted until a later complete or quarantine call.
///
/// # Errors
///
/// Returns [`InboxConsumptionPersistenceError`] for a non-fresh domain state,
/// missing inbox receipt, unsupported isolation, conflicting replay, or a
/// database failure.
pub fn persist_inbox_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    if !matches!(
        (
            consumption.state(),
            consumption.fencing_token(),
            consumption.claim_expires_at_unix_ms().is_none(),
            consumption.completion_evidence_ref(),
            consumption.cause_code()
        ),
        (ConsumptionState::Pending, 0, true, None, None)
    ) {
        return Err(InboxConsumptionPersistenceError::UnsupportedInitialState);
    }
    require_read_committed(transaction)?;
    let latest_event_at_unix_ms = postgres_bigint(consumption.latest_event_at_unix_ms())?;
    let inbox_exists: bool = transaction
        .query_one(
            "SELECT EXISTS (\
                 SELECT 1 FROM integration_inbox \
                 WHERE consumer_ref = $1 AND source_ref = $2 \
                   AND tenant_ref = $3 AND source_event_ref = $4\
             )",
            &[
                &consumption.consumer_ref(),
                &consumption.source_ref(),
                &consumption.tenant_ref(),
                &consumption.source_event_ref(),
            ],
        )?
        .get(0);
    if !inbox_exists {
        return Err(InboxConsumptionPersistenceError::InboxNotFound);
    }

    let inserted = match transaction.execute(
        "INSERT INTO integration_consumption (\
             consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref,\
             side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, 'pending', 0, $7) \
         ON CONFLICT (consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref) \
         DO NOTHING",
        &[
            &consumption.consumer_ref(),
            &consumption.source_ref(),
            &consumption.tenant_ref(),
            &consumption.source_event_ref(),
            &consumption.consumption_ref(),
            &consumption.side_effect_ref(),
            &latest_event_at_unix_ms,
        ],
    ) {
        Ok(inserted) => inserted,
        Err(error) if is_unique_violation(&error) => {
            return Err(InboxConsumptionPersistenceError::ConflictingReplay);
        }
        Err(error) => return Err(error.into()),
    };
    if inserted == 1 {
        return Ok(InboxConsumptionDisposition::Inserted);
    }
    classify_existing_consumption(transaction, consumption)
}

/// Claim one pending consumption and issue a new fencing token.
/// A processing row cannot be stolen. Expire-and-reclaim returns an expired
/// claim to pending without transferring the crashed worker's fence; the next
/// claim increments the stored token.
///
/// # Errors
///
/// Returns [`InboxConsumptionPersistenceError`] for invalid identity or time,
/// an empty claim window, unsupported isolation, a missing or non-pending row,
/// or a database failure.
pub fn begin_inbox_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
    observed_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> Result<u64, InboxConsumptionPersistenceError> {
    let observed_at_unix_ms = require_timestamp(observed_at_unix_ms)?;
    let expires_at_unix_ms = require_timestamp(expires_at_unix_ms)?;
    if expires_at_unix_ms <= observed_at_unix_ms {
        return Err(InboxConsumptionPersistenceError::InvalidConsumptionClaimWindow);
    }
    require_read_committed(transaction)?;
    let row = lock_consumption(transaction, consumption)?;
    require_side_effect_binding(&row, consumption)?;
    let state = parse_consumption_state(row.get(1))?;
    let latest_event_at_unix_ms: i64 = row.get(3);
    match state {
        ConsumptionState::Pending => {}
        ConsumptionState::Processing => {
            return Err(InboxConsumptionPersistenceError::ConsumptionNotClaimable);
        }
        ConsumptionState::Completed | ConsumptionState::Quarantined => {
            return Err(InboxConsumptionPersistenceError::TerminalConsumptionState);
        }
    }
    if observed_at_unix_ms < latest_event_at_unix_ms {
        return Err(InboxConsumptionPersistenceError::NonMonotonicTimestamp);
    }
    let fencing_token: i64 = transaction
        .query_one(
            "UPDATE integration_consumption \
             SET consumption_state = 'processing', \
                 fencing_token = fencing_token + 1, \
                 latest_event_at_unix_ms = $6, \
                 claim_expires_at_unix_ms = $7 \
             WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
               AND source_event_ref = $4 AND consumption_ref = $5 \
               AND consumption_state = 'pending' \
             RETURNING fencing_token",
            &[
                &consumption.consumer_ref(),
                &consumption.source_ref(),
                &consumption.tenant_ref(),
                &consumption.source_event_ref(),
                &consumption.consumption_ref(),
                &observed_at_unix_ms,
                &expires_at_unix_ms,
            ],
        )?
        .get(0);
    u64::try_from(fencing_token).map_err(|_| InboxConsumptionPersistenceError::ValueOutOfRange)
}

/// Recover an expired processing claim without transferring its fence.
///
/// The row returns to pending and keeps the last issued fencing token. A later
/// [`begin_inbox_consumption`] increments that token. The expired worker cannot
/// complete or quarantine with the old fence.
///
/// # Errors
///
/// Returns [`InboxConsumptionPersistenceError`] for invalid identity or time,
/// unsupported isolation, a missing row, a claim that is not processing, a
/// still-live claim, or a database failure.
pub fn expire_inbox_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
    observed_at_unix_ms: u64,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    let observed_at_unix_ms = require_timestamp(observed_at_unix_ms)?;
    require_read_committed(transaction)?;
    let row = lock_consumption(transaction, consumption)?;
    require_side_effect_binding(&row, consumption)?;
    let state = parse_consumption_state(row.get(1))?;
    if state != ConsumptionState::Processing {
        return Err(InboxConsumptionPersistenceError::ConsumptionNotProcessing);
    }
    let claim_expires_at_unix_ms: i64 = row.get(6);
    if observed_at_unix_ms < claim_expires_at_unix_ms {
        return Err(InboxConsumptionPersistenceError::ConsumptionClaimStillActive);
    }

    let updated = transaction.execute(
        "UPDATE integration_consumption \
         SET consumption_state = 'pending', \
             claim_expires_at_unix_ms = NULL, \
             latest_event_at_unix_ms = $6 \
         WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
           AND source_event_ref = $4 AND consumption_ref = $5 \
           AND consumption_state = 'processing' \
           AND claim_expires_at_unix_ms <= $6",
        &[
            &consumption.consumer_ref(),
            &consumption.source_ref(),
            &consumption.tenant_ref(),
            &consumption.source_event_ref(),
            &consumption.consumption_ref(),
            &observed_at_unix_ms,
        ],
    )?;
    if updated == 1 {
        Ok(InboxConsumptionDisposition::Inserted)
    } else {
        Err(InboxConsumptionPersistenceError::InvalidStoredState)
    }
}

/// Persist verified side-effect completion for a pending or claimed consumption.
///
/// Local effects complete from pending with fencing token `0`. Claimed workers
/// must present the current fence. Exact completed replay is idempotent.
///
/// # Errors
///
/// Returns [`InboxConsumptionPersistenceError`] for invalid evidence, a missing
/// row, a stale fence, conflicting completion, quarantine, or a database failure.
pub fn complete_inbox_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
    observed_at_unix_ms: u64,
    completion_evidence_ref: &str,
    expected_fence: u64,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    apply_terminal_transition(
        transaction,
        consumption,
        observed_at_unix_ms,
        expected_fence,
        TerminalTransition::Complete {
            completion_evidence_ref,
        },
    )
}

/// Persist a poison or operator-required quarantine for one consumption.
///
/// Exact quarantine replay is idempotent. A completed consumption cannot be
/// quarantined, and quarantine never invents completion evidence.
///
/// # Errors
///
/// Returns [`InboxConsumptionPersistenceError`] for invalid cause evidence, a
/// missing row, a stale fence, conflicting quarantine, completion, or a
/// database failure.
pub fn quarantine_inbox_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
    observed_at_unix_ms: u64,
    cause_code: &str,
    expected_fence: u64,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    apply_terminal_transition(
        transaction,
        consumption,
        observed_at_unix_ms,
        expected_fence,
        TerminalTransition::Quarantine { cause_code },
    )
}

#[derive(Clone, Copy)]
enum TerminalTransition<'a> {
    Complete { completion_evidence_ref: &'a str },
    Quarantine { cause_code: &'a str },
}

fn apply_terminal_transition(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
    observed_at_unix_ms: u64,
    expected_fence: u64,
    transition: TerminalTransition<'_>,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    let observed_at_unix_ms = require_timestamp(observed_at_unix_ms)?;
    let expected_fence = postgres_bigint(expected_fence)?;
    let (target_state, evidence_value, statement) = match transition {
        TerminalTransition::Complete {
            completion_evidence_ref,
        } => (
            ConsumptionState::Completed,
            required_reference(completion_evidence_ref)?,
            "UPDATE integration_consumption \
             SET consumption_state = 'completed', \
                 completion_evidence_ref = $6, \
                 fencing_token = $8, \
                 claim_expires_at_unix_ms = NULL, \
                 latest_event_at_unix_ms = $7 \
             WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
               AND source_event_ref = $4 AND consumption_ref = $5 \
               AND ( \
                    (consumption_state = 'pending' AND $8 = 0::BIGINT) \
                    OR (consumption_state = 'processing' AND fencing_token = $8) \
               )",
        ),
        TerminalTransition::Quarantine { cause_code } => (
            ConsumptionState::Quarantined,
            required_reference(cause_code)?,
            "UPDATE integration_consumption \
             SET consumption_state = 'quarantined', \
                 cause_code = $6, \
                 fencing_token = $8, \
                 claim_expires_at_unix_ms = NULL, \
                 latest_event_at_unix_ms = $7 \
             WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
               AND source_event_ref = $4 AND consumption_ref = $5 \
               AND ( \
                    (consumption_state = 'pending' AND $8 = 0::BIGINT) \
                    OR (consumption_state = 'processing' AND fencing_token = $8) \
               )",
        ),
    };
    require_read_committed(transaction)?;
    let row = lock_consumption(transaction, consumption)?;
    require_side_effect_binding(&row, consumption)?;
    let state = parse_consumption_state(row.get(1))?;
    let fencing_token: i64 = row.get(2);
    let latest_event_at_unix_ms: i64 = row.get(3);
    let stored_completion: Option<String> = row.get(4);
    let stored_cause: Option<String> = row.get(5);
    let stored_evidence = if target_state == ConsumptionState::Completed {
        stored_completion.as_deref()
    } else {
        stored_cause.as_deref()
    };

    if state == target_state {
        return if stored_evidence == Some(evidence_value)
            && latest_event_at_unix_ms == observed_at_unix_ms
            && fencing_token == expected_fence
        {
            Ok(InboxConsumptionDisposition::Duplicate)
        } else {
            Err(InboxConsumptionPersistenceError::ConflictingReplay)
        };
    }
    if matches!(
        state,
        ConsumptionState::Completed | ConsumptionState::Quarantined
    ) {
        return Err(InboxConsumptionPersistenceError::TerminalConsumptionState);
    }
    let fence_authorized = if state == ConsumptionState::Pending {
        expected_fence == 0
    } else {
        fencing_token == expected_fence
    };
    if !fence_authorized {
        return Err(InboxConsumptionPersistenceError::StaleConsumptionFence);
    }
    if observed_at_unix_ms < latest_event_at_unix_ms {
        return Err(InboxConsumptionPersistenceError::NonMonotonicTimestamp);
    }

    let updated = transaction.execute(
        statement,
        &[
            &consumption.consumer_ref(),
            &consumption.source_ref(),
            &consumption.tenant_ref(),
            &consumption.source_event_ref(),
            &consumption.consumption_ref(),
            &evidence_value,
            &observed_at_unix_ms,
            &expected_fence,
        ],
    )?;
    if updated == 1 {
        Ok(InboxConsumptionDisposition::Inserted)
    } else {
        Err(InboxConsumptionPersistenceError::InvalidStoredState)
    }
}

fn classify_existing_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
) -> Result<InboxConsumptionDisposition, InboxConsumptionPersistenceError> {
    let row = transaction.query_one(
        "SELECT side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms \
         FROM integration_consumption \
         WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
           AND source_event_ref = $4 AND consumption_ref = $5",
        &[
            &consumption.consumer_ref(),
            &consumption.source_ref(),
            &consumption.tenant_ref(),
            &consumption.source_event_ref(),
            &consumption.consumption_ref(),
        ],
    )?;
    let stored_side_effect: String = row.get(0);
    let stored_state = parse_consumption_state(row.get(1))?;
    let stored_fence: i64 = row.get(2);
    let stored_latest: i64 = row.get(3);
    let expected_latest = postgres_bigint(consumption.latest_event_at_unix_ms())?;
    if stored_side_effect == consumption.side_effect_ref()
        && stored_state == ConsumptionState::Pending
        && stored_fence == 0
        && stored_latest == expected_latest
    {
        Ok(InboxConsumptionDisposition::Duplicate)
    } else {
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    }
}

fn lock_consumption(
    transaction: &mut Transaction<'_>,
    consumption: &InboxConsumption,
) -> Result<postgres::Row, InboxConsumptionPersistenceError> {
    transaction
        .query_opt(
            "SELECT side_effect_ref, consumption_state, fencing_token, latest_event_at_unix_ms, \
                    completion_evidence_ref, cause_code, \
                    COALESCE(claim_expires_at_unix_ms, 0) \
             FROM integration_consumption \
             WHERE consumer_ref = $1 AND source_ref = $2 AND tenant_ref = $3 \
               AND source_event_ref = $4 AND consumption_ref = $5 \
             FOR UPDATE",
            &[
                &consumption.consumer_ref(),
                &consumption.source_ref(),
                &consumption.tenant_ref(),
                &consumption.source_event_ref(),
                &consumption.consumption_ref(),
            ],
        )?
        .ok_or(InboxConsumptionPersistenceError::ConsumptionNotFound)
}

fn require_side_effect_binding(
    row: &postgres::Row,
    consumption: &InboxConsumption,
) -> Result<(), InboxConsumptionPersistenceError> {
    let stored_side_effect_ref: String = row.get(0);
    if stored_side_effect_ref == consumption.side_effect_ref() {
        Ok(())
    } else {
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    }
}

fn parse_consumption_state(
    state: &str,
) -> Result<ConsumptionState, InboxConsumptionPersistenceError> {
    match state {
        "pending" => Ok(ConsumptionState::Pending),
        "processing" => Ok(ConsumptionState::Processing),
        "completed" => Ok(ConsumptionState::Completed),
        "quarantined" => Ok(ConsumptionState::Quarantined),
        _ => Err(InboxConsumptionPersistenceError::InvalidStoredState),
    }
}

fn required_reference(reference: &str) -> Result<&str, InboxConsumptionPersistenceError> {
    normalized_reference(reference).ok_or(InboxConsumptionPersistenceError::InvalidReference)
}

fn require_timestamp(timestamp: u64) -> Result<i64, InboxConsumptionPersistenceError> {
    if timestamp == 0 {
        return Err(InboxConsumptionPersistenceError::InvalidTimestamp);
    }
    postgres_bigint(timestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), InboxConsumptionPersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(InboxConsumptionPersistenceError::UnsupportedIsolationLevel)
    }
}

fn postgres_bigint(value: u64) -> Result<i64, InboxConsumptionPersistenceError> {
    i64::try_from(value).map_err(|_| InboxConsumptionPersistenceError::ValueOutOfRange)
}

fn is_unique_violation(error: &postgres::Error) -> bool {
    error.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION)
}

#[cfg(test)]
mod tests {
    use super::{is_unique_violation, parse_consumption_state, InboxConsumptionPersistenceError};
    use crate::integration::ConsumptionState;

    #[test]
    fn stored_consumption_state_parser_is_fail_closed() {
        assert_eq!(
            parse_consumption_state("pending").unwrap(),
            ConsumptionState::Pending
        );
        assert_eq!(
            parse_consumption_state("processing").unwrap(),
            ConsumptionState::Processing
        );
        assert_eq!(
            parse_consumption_state("completed").unwrap(),
            ConsumptionState::Completed
        );
        assert_eq!(
            parse_consumption_state("quarantined").unwrap(),
            ConsumptionState::Quarantined
        );
        assert!(matches!(
            parse_consumption_state("unexpected"),
            Err(InboxConsumptionPersistenceError::InvalidStoredState)
        ));
    }

    #[test]
    fn non_database_postgres_error_is_not_a_unique_violation() {
        let error = postgres::Client::connect(
            "host=127.0.0.1 port=1 user=x dbname=x connect_timeout=1",
            postgres::NoTls,
        )
        .err()
        .expect("a closed loopback port must fail without a PostgreSQL database error");
        assert!(!is_unique_violation(&error));
    }
}
