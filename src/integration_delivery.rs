//! Verified handoff from outbound publication to fenced delivery persistence.
//!
//! Outbound I/O and durable attempt recording remain separate operations. A caller
//! claims and commits an outbox lease, performs outbound publication without holding
//! a database transaction open, and then opens a fresh transaction to persist the
//! verified result. [`VerifiedIntegrationPublishReceipt`] removes the independent
//! identity and outcome arguments that could otherwise be rebound after publication.

use crate::integration::{DeliveryOutcome, IntegrationEvent};
use crate::integration_publisher::{
    execute_integration_publish, IntegrationPublishReceipt, IntegrationPublisher,
    IntegrationPublisherExecutionError,
};
use crate::postgres_integration::{
    record_leased_outbox_delivery_attempt, DeliveryAttemptPersistence, OutboxPersistenceIdentity,
    PersistenceError,
};
use postgres::Transaction;

/// Publisher acknowledgement verified against the exact event that was dispatched.
///
/// Construction is private to this module. Callers can inspect the acknowledged
/// identity and outcome, but they cannot manufacture a verified handoff from an
/// arbitrary raw publisher receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIntegrationPublishReceipt {
    receipt: IntegrationPublishReceipt,
}

impl VerifiedIntegrationPublishReceipt {
    /// Return the verified bounded-context source identity.
    #[must_use]
    pub fn source_ref(&self) -> &str {
        self.receipt.source_ref()
    }

    /// Return the verified tenant identity.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        self.receipt.tenant_ref()
    }

    /// Return the verified immutable event identity.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        self.receipt.event_ref()
    }

    /// Return the verified publisher delivery classification.
    #[must_use]
    pub fn outcome(&self) -> DeliveryOutcome {
        self.receipt.outcome()
    }
}

/// Publish one immutable event and mint a persistence handoff only after exact
/// acknowledgement-identity verification.
///
/// This function performs outbound publication only. It does not hold or open a
/// database transaction and does not record delivery evidence.
///
/// # Errors
///
/// Returns [`IntegrationPublisherExecutionError::Publisher`] when the publisher
/// fails, or [`IntegrationPublisherExecutionError::EventMismatch`] when its receipt
/// acknowledges another source, tenant, or event.
pub fn execute_verified_integration_publish<P>(
    publisher: &P,
    integration_event: &IntegrationEvent,
) -> Result<VerifiedIntegrationPublishReceipt, IntegrationPublisherExecutionError<P::Error>>
where
    P: IntegrationPublisher,
{
    let receipt = execute_integration_publish(publisher, integration_event)?;
    Ok(VerifiedIntegrationPublishReceipt { receipt })
}

/// Persist one verified publisher result under its exact fenced outbox identity.
///
/// The durable identity and delivery outcome are derived from the verified receipt;
/// callers supply only attempt evidence and the fencing token obtained from the
/// outbox lease. Call this in a fresh transaction after outbound I/O has completed.
///
/// # Errors
///
/// Returns [`PersistenceError`] when the attempt evidence, current lease, fencing
/// token, database clock, replay evidence, or durable outbox state fails validation.
pub fn record_verified_leased_delivery_attempt(
    transaction: &mut Transaction<'_>,
    verified_receipt: &VerifiedIntegrationPublishReceipt,
    attempt_ref: &str,
    occurred_at_unix_ms: u64,
    cause_code: Option<&str>,
    fencing_token: u64,
) -> Result<DeliveryAttemptPersistence, PersistenceError> {
    record_leased_outbox_delivery_attempt(
        transaction,
        OutboxPersistenceIdentity::new(
            verified_receipt.source_ref(),
            verified_receipt.tenant_ref(),
            verified_receipt.event_ref(),
        ),
        attempt_ref,
        verified_receipt.outcome(),
        occurred_at_unix_ms,
        cause_code,
        fencing_token,
    )
}
