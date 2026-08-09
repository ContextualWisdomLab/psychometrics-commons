//! Regression tests for exact instrument-release binding during item delivery.

use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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

fn request<'a>(item_version_ref: &'a str) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_event_001",
        item_version_ref,
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    }
}

#[test]
fn ledger_derives_release_identity_locale_and_item_set_from_exact_manifest() {
    let manifest = manifest();
    let ledger = ItemDeliveryLedger::from_manifest("session_big_five_001", &manifest).unwrap();

    assert_eq!(ledger.session_ref(), "session_big_five_001");
    assert_eq!(ledger.instrument_release_ref(), manifest.release_ref());
    assert_eq!(ledger.release_content_digest(), manifest.content_digest());
    assert_eq!(ledger.locale(), manifest.locale());
    assert_eq!(ledger.allowed_item_version_refs(), manifest.item_version_refs());
}

#[test]
fn item_not_present_in_exact_release_manifest_cannot_be_delivered() {
    let manifest = manifest();
    let mut ledger = ItemDeliveryLedger::from_manifest("session_big_five_001", &manifest).unwrap();

    assert_eq!(
        ledger.deliver(SessionState::Active, request("item_version_outside_release")),
        Err(ItemDeliveryError::ItemNotInRelease)
    );
    assert!(ledger.is_empty());
}
