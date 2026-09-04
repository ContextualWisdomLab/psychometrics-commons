//! Regression tests for exact instrument-release binding during item delivery.

mod item_delivery_support;

use item_delivery_support::{published_release, session_in_state};
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

fn request(item_version_ref: &str) -> ItemDeliveryRequest<'_> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_event_001",
        item_version_ref,
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    }
}

#[test]
fn ledger_derives_release_identity_locale_and_item_set_from_exact_session_manifest() {
    let release = published_release();
    let session = session_in_state(&release, SessionState::Created);
    let manifest = release.manifest();
    let ledger = ItemDeliveryLedger::from_session(&session, manifest).unwrap();

    assert_eq!(ledger.session_ref(), session.session_ref());
    assert_eq!(ledger.instrument_release_ref(), manifest.release_ref());
    assert_eq!(
        ledger.instrument_version_ref(),
        manifest.instrument_version_ref()
    );
    assert_eq!(ledger.release_content_digest(), manifest.content_digest());
    assert_eq!(ledger.locale(), manifest.locale());
    assert_eq!(
        ledger.allowed_item_version_refs(),
        manifest.item_version_refs()
    );
}

#[test]
fn item_not_present_in_exact_release_manifest_cannot_be_delivered() {
    let release = published_release();
    let session = session_in_state(&release, SessionState::Active);
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();

    assert_eq!(
        ledger.deliver(&session, request("item_version_outside_release")),
        Err(ItemDeliveryError::ItemNotInRelease)
    );
    assert!(ledger.is_empty());
}
