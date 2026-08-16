//! Domain contract: already-shown items survive process restart.
//!
//! A buyer mid-assessment must not see a previously delivered item again, and
//! the runtime must not skip ahead, after the process reloads stored evidence.
//! Reconstruction uses stored header fields plus contiguous delivery sequence
//! and does not require the session to still be Active.

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

fn request<'a>(
    delivery_ref: &'a str,
    item_version_ref: &'a str,
    presentation_context_ref: &'a str,
    selection_evidence_ref: Option<&'a str>,
) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref,
        item_version_ref,
        presentation_context_ref,
        selection_evidence_ref,
    }
}

#[test]
fn persisted_header_reloads_the_same_empty_ledger_as_the_original_manifest() {
    let original =
        ItemDeliveryLedger::from_manifest("session_item_reload_empty", &manifest()).unwrap();
    let restored = ItemDeliveryLedger::from_persisted(
        original.session_ref(),
        original.instrument_release_ref(),
        original.release_content_digest(),
        original.locale(),
        original
            .allowed_item_version_refs()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .unwrap();

    assert_eq!(restored, original);
    assert!(restored.is_empty());
}

#[test]
fn restored_events_keep_server_order_without_requiring_an_active_session() {
    let mut live =
        ItemDeliveryLedger::from_manifest("session_item_reload_order", &manifest()).unwrap();
    live.deliver(
        SessionState::Active,
        request(
            "delivery_event_002",
            "item_version_002",
            "presentation_standard_v1",
            Some("selection_adaptive_v1"),
        ),
    )
    .unwrap();
    live.deliver(
        SessionState::Active,
        request(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        ),
    )
    .unwrap();

    let mut restored = ItemDeliveryLedger::from_persisted(
        "session_item_reload_order",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap();
    restored
        .restore_persisted_event(
            request(
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("selection_adaptive_v1"),
            ),
            1,
        )
        .unwrap();
    restored
        .restore_persisted_event(
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            2,
        )
        .unwrap();

    assert_eq!(restored, live);
    assert_eq!(restored.events()[0].item_version_ref(), "item_version_002");
    assert_eq!(restored.events()[1].item_version_ref(), "item_version_001");
    assert_eq!(restored.events()[0].sequence(), 1);
    assert_eq!(restored.events()[1].sequence(), 2);
}

#[test]
fn sequence_gap_or_foreign_item_fails_closed_instead_of_skipping_ahead() {
    let mut restored = ItemDeliveryLedger::from_persisted(
        "session_item_reload_corrupt",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap();

    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                None,
            ),
            2,
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_003",
                "item_version_003",
                "presentation_standard_v1",
                None,
            ),
            1,
        ),
        Err(ItemDeliveryError::ItemNotInRelease)
    );
    assert!(restored.is_empty());
}

#[test]
fn persisted_header_rejects_blank_digest_locale_or_empty_allowed_set() {
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_bad_digest",
            "release_big_five_ko_v1",
            "sha256:not-a-digest",
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_padded_locale",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            " ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_empty_allowed",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &[],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "12",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_duplicate_allowed",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001", "item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_md5_digest",
            "release_big_five_ko_v1",
            "md5:0123456789abcdef0123456789abcdef",
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_short_locale",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "k",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_bad_subtag",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR!",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
}

#[test]
fn restore_rejects_repeated_identity_or_malformed_evidence() {
    let mut restored = ItemDeliveryLedger::from_persisted(
        "session_item_reload_repeat",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap();
    restored
        .restore_persisted_event(
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            1,
        )
        .unwrap();

    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_001",
                "item_version_002",
                "presentation_standard_v1",
                None,
            ),
            2,
        ),
        Err(ItemDeliveryError::CorruptHistory)
    );
    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_002",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            2,
        ),
        Err(ItemDeliveryError::DuplicateItemDelivery)
    );
    assert_eq!(
        restored.restore_persisted_event(
            request("12", "item_version_002", "presentation_standard_v1", None),
            2,
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("12"),
            ),
            2,
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_numeric_item",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["12"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
}

#[test]
fn persisted_reconstruction_rejects_padded_identities_instead_of_trimming() {
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            " session_item_reload_padded",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_padded_release",
            " release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &["item_version_001"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        ItemDeliveryLedger::from_persisted(
            "session_item_reload_padded_item",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            "ko-KR",
            &[" item_version_001"],
        ),
        Err(ItemDeliveryError::InvalidReference)
    );

    let mut restored = ItemDeliveryLedger::from_persisted(
        "session_item_reload_padded_event",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap();
    assert_eq!(
        restored.restore_persisted_event(
            request(
                " delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            1,
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert_eq!(
        restored.restore_persisted_event(
            request(
                "delivery_event_001",
                "item_version_001",
                " presentation_standard_v1",
                None,
            ),
            1,
        ),
        Err(ItemDeliveryError::InvalidReference)
    );
    assert!(restored.is_empty());
}

#[test]
fn restored_ledger_continues_delivery_without_re_presenting_shown_items() {
    let mut restored = ItemDeliveryLedger::from_persisted(
        "session_item_reload_continue",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
        &["item_version_001", "item_version_002"],
    )
    .unwrap();
    restored
        .restore_persisted_event(
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            1,
        )
        .unwrap();

    assert_eq!(
        restored.deliver(
            SessionState::Active,
            request(
                "delivery_event_repeat",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        ),
        Err(ItemDeliveryError::DuplicateItemDelivery)
    );
    let next = restored
        .deliver(
            SessionState::Active,
            request(
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                None,
            ),
        )
        .unwrap();
    assert_eq!(next.item_version_ref(), "item_version_002");
    assert_eq!(next.sequence(), 2);
    assert_eq!(restored.len(), 2);
}
