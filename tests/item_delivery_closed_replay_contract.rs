//! Regression tests for item-delivery replay after collection stops accepting new items.

use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn request<'a>(item_version_ref: &'a str, presentation_context_ref: &'a str) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_event_001",
        item_version_ref,
        presentation_context_ref,
        selection_evidence_ref: Some("selection_fixed_order_v1"),
    }
}

fn ledger_with_delivery() -> ItemDeliveryLedger {
    let mut ledger = ItemDeliveryLedger::new(
        "session_big_five_001",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
    )
    .unwrap();
    ledger
        .deliver(
            SessionState::Active,
            request("item_version_001", "presentation_standard_v1"),
        )
        .unwrap();
    ledger
}

#[test]
fn conflicting_replay_after_collection_closes_is_not_misclassified_as_new_delivery() {
    for state in [
        SessionState::Paused,
        SessionState::Completed,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        let mut ledger = ledger_with_delivery();
        assert_eq!(
            ledger.deliver(
                state,
                request("item_version_002", "presentation_standard_v1")
            ),
            Err(ItemDeliveryError::IdempotencyConflict),
            "conflicting replay must remain fail-closed after state {state:?}"
        );
        assert_eq!(ledger.len(), 1);
    }
}

#[test]
fn genuinely_new_delivery_after_collection_closes_is_still_rejected_by_session_state() {
    let mut ledger = ledger_with_delivery();
    let new_delivery = ItemDeliveryRequest {
        delivery_ref: "delivery_event_002",
        item_version_ref: "item_version_002",
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    };

    assert_eq!(
        ledger.deliver(SessionState::Completed, new_delivery),
        Err(ItemDeliveryError::SessionNotActive(SessionState::Completed))
    );
    assert_eq!(ledger.len(), 1);
}
