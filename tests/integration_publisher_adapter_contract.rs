//! Contract tests for the outbound integration publisher adapter boundary.

use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent};
use psychometrics_commons_runtime::integration_publisher::{
    execute_integration_publish, IntegrationPublishReceipt, IntegrationPublisher,
    IntegrationPublisherExecutionError,
};
use std::error::Error;
use std::fmt::{Display, Formatter};

const EVENT_PRIMARY: &str = "evt_3b685ee20bcf448e8acd2b1ce2d5d64e";
const EVENT_OTHER: &str = "evt_b7bf5b281e534122968e259ddd82a532";
const TENANT_PRIMARY: &str = "tnt_7e8d22c33d1646448aa0f8acb5ba2c90";
const TENANT_OTHER: &str = "tnt_4a98421fca8644959240ed2f3301f0f6";
const SOURCE_PRIMARY: &str = "src_197fdf358dde485e89c627dde386b131";
const SOURCE_OTHER: &str = "src_945b40e30d264888a97a364fd1c64e17";
const SUBJECT_REF: &str = "rsrc_2c692aa8dd4f4e10ad4862c8e0e49f87";
const CORRELATION_REF: &str = "cor_b6617915efaf482383545c94437eed86";

fn event_with_identity(event_ref: &str, tenant_ref: &str, source_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "result.released",
        "v1",
        source_ref,
        tenant_ref,
        SUBJECT_REF,
        1_786_240_000_000,
        CORRELATION_REF,
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

fn event(event_ref: &str, tenant_ref: &str) -> IntegrationEvent {
    event_with_identity(event_ref, tenant_ref, SOURCE_PRIMARY)
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

struct ClassifiedPublisher {
    outcome: DeliveryOutcome,
}

impl IntegrationPublisher for ClassifiedPublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            integration_event,
            self.outcome,
        ))
    }
}

struct MismatchedPublisher {
    acknowledged_event: IntegrationEvent,
}

impl IntegrationPublisher for MismatchedPublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        _integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            &self.acknowledged_event,
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
    let integration_event = event(EVENT_PRIMARY, TENANT_PRIMARY);
    let receipt = execute_integration_publish(&SuccessfulPublisher, &integration_event).unwrap();

    assert_eq!(receipt.source_ref(), SOURCE_PRIMARY);
    assert_eq!(receipt.tenant_ref(), TENANT_PRIMARY);
    assert_eq!(receipt.event_ref(), EVENT_PRIMARY);
    assert_eq!(receipt.outcome(), DeliveryOutcome::Delivered);
}

#[test]
fn receipt_value_semantics_preserve_exact_identity_and_outcome() {
    let primary = IntegrationPublishReceipt::for_event(
        &event(EVENT_PRIMARY, TENANT_PRIMARY),
        DeliveryOutcome::Delivered,
    );
    let cloned = primary.clone();
    let other = IntegrationPublishReceipt::for_event(
        &event(EVENT_OTHER, TENANT_PRIMARY),
        DeliveryOutcome::Delivered,
    );

    assert_eq!(cloned, primary);
    assert_ne!(other, primary);
    let debug = format!("{cloned:?}");
    assert!(debug.contains(EVENT_PRIMARY));
    assert!(debug.contains(TENANT_PRIMARY));
}

#[test]
fn adapter_preserves_each_publisher_delivery_classification() {
    let integration_event = event(EVENT_PRIMARY, TENANT_PRIMARY);

    for outcome in [
        DeliveryOutcome::Delivered,
        DeliveryOutcome::RetryableFailure,
        DeliveryOutcome::PermanentFailure,
    ] {
        let receipt =
            execute_integration_publish(&ClassifiedPublisher { outcome }, &integration_event)
                .unwrap();

        assert_eq!(receipt.source_ref(), SOURCE_PRIMARY);
        assert_eq!(receipt.tenant_ref(), TENANT_PRIMARY);
        assert_eq!(receipt.event_ref(), EVENT_PRIMARY);
        assert_eq!(receipt.outcome(), outcome);
    }
}

#[test]
fn adapter_rejects_each_independent_outbox_identity_rebinding() {
    let integration_event = event(EVENT_PRIMARY, TENANT_PRIMARY);
    let mismatches = [
        event_with_identity(EVENT_PRIMARY, TENANT_PRIMARY, SOURCE_OTHER),
        event(EVENT_PRIMARY, TENANT_OTHER),
        event(EVENT_OTHER, TENANT_PRIMARY),
    ];

    for acknowledged_event in mismatches {
        let error = execute_integration_publish(
            &MismatchedPublisher { acknowledged_event },
            &integration_event,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            IntegrationPublisherExecutionError::EventMismatch
        ));
        assert_eq!(
            error.to_string(),
            "integration publisher receipt does not belong to the dispatched event"
        );
        assert!(error.source().is_none());
        assert!(format!("{error:?}").contains("EventMismatch"));
    }
}

#[test]
fn adapter_preserves_publisher_failure_as_the_error_source() {
    let integration_event = event(EVENT_PRIMARY, TENANT_PRIMARY);
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
    assert!(format!("{error:?}").contains("PublisherUnavailable"));
}
