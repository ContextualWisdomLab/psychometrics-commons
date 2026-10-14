//! Regression tests for item-delivery replay after collection stops accepting new items.

mod item_delivery_support;

use item_delivery_support::{published_release, session_in_state};
use psychometrics_commons_runtime::instrument::InstrumentRelease;
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

fn request<'a>(
    item_version_ref: &'a str,
    presentation_context_ref: &'a str,
) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_event_001",
        item_version_ref,
        presentation_context_ref,
        selection_evidence_ref: Some("selection_fixed_order_v1"),
    }
}

fn ledger_with_delivery() -> (InstrumentRelease, ItemDeliveryLedger) {
    let release = published_release();
    let session = session_in_state(&release, SessionState::Active);
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    ledger
        .deliver(
            &session,
            request("item_version_001", "presentation_standard_v1"),
        )
        .unwrap();
    (release, ledger)
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
        let (release, mut ledger) = ledger_with_delivery();
        let closed_session = session_in_state(&release, state);
        assert_eq!(
            ledger.deliver(
                &closed_session,
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
    let (release, mut ledger) = ledger_with_delivery();
    let completed_session = session_in_state(&release, SessionState::Completed);
    let new_delivery = ItemDeliveryRequest {
        delivery_ref: "delivery_event_002",
        item_version_ref: "item_version_002",
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    };

    assert_eq!(
        ledger.deliver(&completed_session, new_delivery),
        Err(ItemDeliveryError::SessionNotActive(SessionState::Completed))
    );
    assert_eq!(ledger.len(), 1);
}
