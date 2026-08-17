//! Contract tests for the outbound integration publisher adapter boundary.

use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent};
use psychometrics_commons_runtime::integration_publisher::{
    execute_integration_publish, IntegrationPublishReceipt, IntegrationPublisher,
    IntegrationPublisherExecutionError,
};
use std::error::Error;
use std::fmt::{Display, Formatter};

fn event(event_ref: &str, tenant_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "result.released",
        "v1",
        "psychometrics_commons",
        tenant_ref,
        "result_snapshot_ref",
        1_786_240_000_000,
        "correlation_ref",
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

#[derive(Debug)]
struct PublisherUnavailable;

impl Display for PublisherUnavailable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("egress transport unavailable")
    }
}

impl Error for PublisherUnavailable {}

struct SuccessfulPublisher;

impl IntegrationPublisher for SuccessfulPublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            integration_event,
            DeliveryOutcome::Delivered,
        ))
    }
}

struct MismatchedPublisher;

impl IntegrationPublisher for MismatchedPublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        _integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            &event("event_other", "tenant_other"),
            DeliveryOutcome::Delivered,
        ))
    }
}

struct UnavailablePublisher;

impl IntegrationPublisher for UnavailablePublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        _integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Err(PublisherUnavailable)
    }
}

#[test]
fn adapter_returns_only_a_receipt_bound_to_the_exact_event() {
    let integration_event = event("event_primary", "tenant_primary");
    let receipt = execute_integration_publish(&SuccessfulPublisher, &integration_event).unwrap();

    assert_eq!(receipt.source_ref(), "psychometrics_commons");
    assert_eq!(receipt.tenant_ref(), "tenant_primary");
    assert_eq!(receipt.event_ref(), "event_primary");
    assert_eq!(receipt.outcome(), DeliveryOutcome::Delivered);
}

#[test]
fn adapter_rejects_a_receipt_for_another_event_identity() {
    let integration_event = event("event_primary", "tenant_primary");
    let error = execute_integration_publish(&MismatchedPublisher, &integration_event).unwrap_err();

    assert!(matches!(
        error,
        IntegrationPublisherExecutionError::EventMismatch
    ));
    assert_eq!(
        error.to_string(),
        "integration publisher receipt does not belong to the dispatched event"
    );
    assert!(error.source().is_none());
}

#[test]
fn adapter_preserves_publisher_failure_as_the_error_source() {
    let integration_event = event("event_primary", "tenant_primary");
    let error = execute_integration_publish(&UnavailablePublisher, &integration_event).unwrap_err();

    assert!(matches!(
        error,
        IntegrationPublisherExecutionError::Publisher(_)
    ));
    assert_eq!(error.to_string(), "integration publisher execution failed");
    assert_eq!(
        error.source().map(ToString::to_string),
        Some("egress transport unavailable".to_owned())
    );
}
