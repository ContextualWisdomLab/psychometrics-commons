//! Atomic `PostgreSQL` composition for consent evidence and propagation outbox events.
//!
//! Consent remains purpose-specific, append-only product evidence. This module composes the
//! existing consent adapter with the existing transactional outbox and verifies that the emitted
//! event is bound to the same participant and one exact consent event before any durable write.

use crate::consent::ConsentLedger;
use crate::integration::IntegrationEvent;
use crate::postgres_consent::{
    persist_consent_ledger, ConsentPersistenceDisposition, ConsentPersistenceError,
};
use crate::postgres_integration::{enqueue_outbox_event, PersistenceDisposition, PersistenceError};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const SOURCE_REF: &str = "psychometrics_commons";

/// Durable dispositions produced by one atomic consent/outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsentOutboxPersistence {
    consent: ConsentPersistenceDisposition,
    outbox: PersistenceDisposition,
}

impl ConsentOutboxPersistence {
    /// Return whether consent evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn consent(self) -> ConsentPersistenceDisposition {
        self.consent
    }

    /// Return whether immutable outbox evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.outbox
    }
}

/// Fail-closed error for atomic consent and outbox persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConsentOutboxPersistenceError {
    /// The propagation envelope does not identify this participant and one exact ledger event.
    InvalidPropagationEnvelope,
    /// Durable consent evidence failed validation or persistence.
    Consent(ConsentPersistenceError),
    /// Durable integration outbox evidence failed validation or persistence.
    Outbox(PersistenceError),
}

impl Display for ConsentOutboxPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPropagationEnvelope => {
                "consent propagation event must bind the exact participant and consent event"
            }
            Self::Consent(_) => "consent propagation consent persistence failed",
            Self::Outbox(_) => "consent propagation outbox persistence failed",
        })
    }
}

impl Error for ConsentOutboxPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPropagationEnvelope => None,
            Self::Consent(error) => Some(error),
            Self::Outbox(error) => Some(error),
        }
    }
}

/// Persist one consent ledger and one causally bound outbox event in the same transaction.
///
/// The integration event must be emitted by `psychometrics_commons`, use the consent ledger's
/// participant as its subject, identify one exact consent event through `causation_ref`, and use
/// that consent event's server-authoritative occurrence time. These checks run before persistence,
/// preventing a valid consent decision from being durably paired with unrelated propagation
/// evidence. The event type, tenant, correlation reference, schema version, and payload digest
/// remain owned by the caller's versioned integration contract.
///
/// The caller owns the `READ COMMITTED` transaction and final commit/rollback decision. If either
/// durable adapter fails, callers must roll the transaction back so newly accepted consent evidence
/// cannot survive without its outbox record. Exact replay remains idempotent at both adapters.
///
/// # Errors
///
/// Returns [`ConsentOutboxPersistenceError::InvalidPropagationEnvelope`] before writes for an
/// unrelated source, participant, causation reference, or timestamp. Consent and outbox failures
/// are preserved in typed error variants.
pub fn persist_consent_ledger_with_outbox(
    transaction: &mut Transaction<'_>,
    ledger: &ConsentLedger,
    propagation_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ConsentOutboxPersistence, ConsentOutboxPersistenceError> {
    validate_propagation_envelope(ledger, propagation_event)?;
    let consent = persist_consent_ledger(transaction, ledger)
        .map_err(ConsentOutboxPersistenceError::Consent)?;
    let outbox = enqueue_outbox_event(transaction, propagation_event, outbox_max_attempts)
        .map_err(ConsentOutboxPersistenceError::Outbox)?;
    Ok(ConsentOutboxPersistence { consent, outbox })
}

fn validate_propagation_envelope(
    ledger: &ConsentLedger,
    propagation_event: &IntegrationEvent,
) -> Result<(), ConsentOutboxPersistenceError> {
    if propagation_event.source() != SOURCE_REF
        || propagation_event.subject_ref() != ledger.participant_ref()
    {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    let Some(causation_ref) = propagation_event.causation_ref() else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    let Some(consent_event) = ledger
        .events()
        .iter()
        .find(|event| event.event_ref() == causation_ref)
    else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    if consent_event.occurred_at_unix_ms() != propagation_event.occurred_at_unix_ms() {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    Ok(())
}
