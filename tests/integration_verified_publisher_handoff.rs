//! Contract tests for the verified outbound-publish handoff.

use psychometrics_commons_runtime::integration::{DeliveryOutcome, IntegrationEvent};
use psychometrics_commons_runtime::integration_delivery::execute_verified_integration_publish;
use psychometrics_commons_runtime::integration_publisher::{
    IntegrationPublishReceipt, IntegrationPublisher, IntegrationPublisherExecutionError,
};
use std::error::Error;
use std::fmt::{Display, Formatter};

const EVENT_PRIMARY: &str = "evt_08c2c1a3f02d4d8eb7e3b4d23be7c0a1";
const EVENT_OTHER: &str = "evt_65d8c037a1014aa2bb90f2ce3b71a44e";
const TENANT: &str = "tnt_8f4cd56b90944c10986828715646bca9";
const SOURCE: &str = "src_ae7101265f7647abb8f77b4e28be33a0";
const SUBJECT: &str = "rsrc_834c2d734fb14e349f71cefb21ad68d9";
const CORRELATION: &str = "cor_e92a62357e9b4b67b9f4f8017f00fcec";

fn event(event_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "result.released",
        "v1",
        SOURCE,
        TENANT,
        SUBJECT,
        1_787_200_000_000,
        CORRELATION,
        None,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

#[derive(Debug)]
struct PublisherUnavailable;

impl Display for PublisherUnavailable {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("verified delivery publisher unavailable")
    }
}

impl Error for PublisherUnavailable {}

struct RetryablePublisher;

impl IntegrationPublisher for RetryablePublisher {
    type Error = PublisherUnavailable;

    fn publish(
        &self,
        integration_event: &IntegrationEvent,
    ) -> Result<IntegrationPublishReceipt, Self::Error> {
        Ok(IntegrationPublishReceipt::for_event(
            integration_event,
            DeliveryOutcome::RetryableFailure,
        ))
    }
}

struct RebindingPublisher {
    acknowledged_event: IntegrationEvent,
}

impl IntegrationPublisher for RebindingPublisher {
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

#[test]
fn verified_receipt_preserves_the_exact_dispatched_identity_and_outcome() {
    let integration_event = event(EVENT_PRIMARY);
    let verified = execute_verified_integration_publish(&RetryablePublisher, &integration_event)
        .expect("the exact publisher acknowledgement should verify");

    assert_eq!(verified.source_ref(), SOURCE);
    assert_eq!(verified.tenant_ref(), TENANT);
    assert_eq!(verified.event_ref(), EVENT_PRIMARY);
    assert_eq!(verified.outcome(), DeliveryOutcome::RetryableFailure);
}

#[test]
fn verified_receipt_is_not_minted_for_another_event() {
    let integration_event = event(EVENT_PRIMARY);
    let error = execute_verified_integration_publish(
        &RebindingPublisher {
            acknowledged_event: event(EVENT_OTHER),
        },
        &integration_event,
    )
    .expect_err("a receipt for another immutable event must fail closed");

    assert!(matches!(
        error,
        IntegrationPublisherExecutionError::EventMismatch
    ));
}
