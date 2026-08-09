//! Value-semantics tests for public item-delivery evidence types.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn clone_public_value<T: Clone>(value: &T) -> T {
    value.clone()
}

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_big_five_ko_v1"),
        "narrative_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        RELEASE_DIGEST,
    )
    .unwrap()
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

    let mut ledger =
        ItemDeliveryLedger::from_manifest("session_big_five_001", &manifest()).unwrap();
    let event = ledger.deliver(SessionState::Active, request).unwrap();

    assert_eq!(clone_public_value(&event), event);
    assert_eq!(clone_public_value(&ledger), ledger);
    assert!(format!("{event:?}").contains("delivery_event_001"));
    assert!(format!("{ledger:?}").contains("session_big_five_001"));
}
