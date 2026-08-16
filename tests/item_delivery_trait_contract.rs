//! Value-semantics tests for public item-delivery evidence types.

mod item_delivery_support;

use item_delivery_support::{published_release, session_in_state};
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

fn clone_public_value<T: Clone>(value: &T) -> T {
    value.clone()
}

#[test]
fn cloned_public_values_preserve_delivery_evidence() {
    let request = ItemDeliveryRequest {
        delivery_ref: "delivery_event_001",
        item_version_ref: "item_version_001",
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: Some("selection_fixed_order_v1"),
    };
    assert_eq!(clone_public_value(&request), request);
    assert_eq!(
        clone_public_value(&ItemDeliveryError::SessionNotActive(SessionState::Paused)),
        ItemDeliveryError::SessionNotActive(SessionState::Paused)
    );
    assert_eq!(
        clone_public_value(&ItemDeliveryError::SessionMismatch),
        ItemDeliveryError::SessionMismatch
    );

    let release = published_release();
    let session = session_in_state(&release, SessionState::Active);
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    let event = ledger.deliver(&session, request).unwrap();

    assert_eq!(clone_public_value(&event), event);
    assert_eq!(clone_public_value(&ledger), ledger);
    assert!(format!("{event:?}").contains("delivery_event_001"));
    assert!(format!("{ledger:?}").contains("session_big_five_001"));
}
