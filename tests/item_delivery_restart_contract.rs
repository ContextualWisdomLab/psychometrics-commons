//! Restart contract for already-shown item evidence.
//!
//! Reconstituting a persisted ledger must preserve server order and prevent an
//! already-shown item from being presented again. Reconstruction consumes exact
//! stored evidence; it never invents a score or requires the session to remain
//! Active merely to replay accepted history.

use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn request<'a>(delivery_ref: &'a str, item_version_ref: &'a str) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref,
        item_version_ref,
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    }
}

fn restored() -> ItemDeliveryLedger {
    ItemDeliveryLedger::from_persisted(
        "session_item_restart",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap()
}

#[test]
fn contiguous_persisted_events_restore_and_next_delivery_continues() {
    let mut ledger = restored();
    ledger
        .restore_persisted_event(request("delivery_event_001", "item_version_001"), 1)
        .unwrap();

    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.events()[0].sequence(), 1);
    assert_eq!(
        ledger.deliver(
            SessionState::Active,
            request("delivery_event_repeat", "item_version_001")
        ),
        Err(ItemDeliveryError::DuplicateItemDelivery)
    );

    let next = ledger
        .deliver(
            SessionState::Active,
            request("delivery_event_002", "item_version_002"),
        )
        .unwrap();
    assert_eq!(next.sequence(), 2);
}

#[test]
fn restart_reconstruction_fails_closed_on_sequence_gap_or_foreign_item() {
    let mut ledger = restored();
    assert_eq!(
        ledger.restore_persisted_event(request("delivery_event_002", "item_version_002"), 2),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ledger.restore_persisted_event(request("delivery_event_003", "item_version_003"), 1),
        Err(ItemDeliveryError::ItemNotInRelease)
    );
    assert!(ledger.is_empty());
}

#[test]
fn restart_reconstruction_rejects_noncanonical_or_corrupt_header_evidence() {
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            " session_item_restart",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_restart",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            " ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_restart",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001", "item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
}
