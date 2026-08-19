//! Product-owned boundary for publishing immutable integration events.
//!
//! Psychometrics Commons owns event identity, the durable outbox, and delivery
//! evidence. Here, an **outbox** is a product-owned persisted queue entry for an
//! immutable event waiting to be delivered. **Egress** means network traffic leaving
//! this service through an approved transport and security policy. **Fencing** means
//! checking the current delivery lease/token so a stale worker cannot record an
//! outcome. A **durable delivery attempt** is the persisted record of an attempted
//! delivery and its classification, retained across process restarts.
//!
//! This repository does not own external network-egress policy. Implementations of
//! [`IntegrationPublisher`] may therefore call an approved outbound transport such
//! as an EgressWeave-compatible adapter without moving delivery authority or a
//! cross-service database connection into this repository.

use crate::integration::{DeliveryOutcome, IntegrationEvent};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Immutable acknowledgement returned by an outbound integration publisher.
///
/// The receipt repeats the complete durable outbox identity (the persisted event
/// waiting for delivery) so the product can reject an acknowledgement produced for
/// another source, tenant, or event before recording delivery evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationPublishReceipt {
    source_ref: String,
    tenant_ref: String,
    event_ref: String,
    outcome: DeliveryOutcome,
}

impl IntegrationPublishReceipt {
    /// Build a receipt for the exact immutable event supplied to a publisher.
    #[must_use]
    pub fn for_event(event: &IntegrationEvent, outcome: DeliveryOutcome) -> Self {
        Self {
            source_ref: event.source().to_owned(),
            tenant_ref: event.tenant_ref().to_owned(),
            event_ref: event.event_ref().to_owned(),
            outcome,
        }
    }

    /// Return the bounded-context source identity acknowledged by the publisher.
    #[must_use]
    pub fn source_ref(&self) -> &str {
        &self.source_ref
    }

    /// Return the tenant identity acknowledged by the publisher.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the immutable event identity acknowledged by the publisher.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the publisher's delivery classification.
    #[must_use]
    pub const fn outcome(&self) -> DeliveryOutcome {
        self.outcome
    }

    fn matches_event(&self, event: &IntegrationEvent) -> bool {
        self.source_ref == event.source()
            && self.tenant_ref == event.tenant_ref()
            && self.event_ref == event.event_ref()
    }
}

/// Approved outbound transport boundary for one immutable integration event.
///
/// Implementations own only transport execution. Product retry and quarantine stay
/// in the integration layer. Fencing (rejecting a stale delivery lease/token) and
/// durable delivery-attempt recording (persisting the attempt and outcome across
/// restarts) remain in the integration and `PostgreSQL` adapters.
pub trait IntegrationPublisher {
    /// Typed provider, policy, or transport failure.
    type Error: Error + Send + Sync + 'static;

    /// Publish one immutable event and return an identity-bound acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns the implementation-defined error when the outbound transport cannot
    /// classify the attempted delivery.
    fn publish(
        &self,
        integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error>;
}

/// Fail-closed error from the outbound integration publisher boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum IntegrationPublisherExecutionError<E> {
    /// The outbound publisher could not complete or classify the attempt.
    Publisher(E),
    /// The publisher acknowledged another immutable event identity.
    EventMismatch,
}

impl<E> Display for IntegrationPublisherExecutionError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Publisher(_) => "integration publisher execution failed",
            Self::EventMismatch => {
                "integration publisher receipt does not belong to the dispatched event"
            }
        })
    }
}

impl<E> Error for IntegrationPublisherExecutionError<E>
where
    E: Error + Send + Sync + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Publisher(error) => Some(error),
            Self::EventMismatch => None,
        }
    }
}

/// Publish one immutable event and enforce exact acknowledgement provenance.
///
/// This function does not mutate an outbox entry, retry a request, or bypass egress
/// policy. A caller with a current durable delivery lease can record the returned
/// outcome through the existing fenced `PostgreSQL` delivery-attempt path only after
/// this identity check succeeds. In other words, the caller must still prove that it
/// is the current worker and persist the attempt/outcome using the existing store.
///
/// # Errors
///
/// Returns [`IntegrationPublisherExecutionError::Publisher`] when the outbound
/// transport fails, or [`IntegrationPublisherExecutionError::EventMismatch`] when
/// its acknowledgement belongs to another source, tenant, or event.
pub fn execute_integration_publish<P>(
    publisher: &P,
    integration_event: &IntegrationEvent,
) -> Result<IntegrationPublishReceipt, IntegrationPublisherExecutionError<P::Error>>
where
    P: IntegrationPublisher,
{
    let receipt = publisher
        .publish(integration_event)
        .map_err(IntegrationPublisherExecutionError::Publisher)?;
    if !receipt.matches_event(integration_event) {
        return Err(IntegrationPublisherExecutionError::EventMismatch);
    }
    Ok(receipt)
}
