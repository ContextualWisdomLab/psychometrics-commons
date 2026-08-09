//! Contract tests for session-bound immutable item-delivery evidence.

use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::SessionState;

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn ledger() -> ItemDeliveryLedger {
    ItemDeliveryLedger::new(
        "session_big_five_001",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        "ko-KR",
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
fn ledger_pins_session_release_digest_and_locale() {
    let ledger = ledger();

    assert_eq!(ledger.session_ref(), "session_big_five_001");
    assert_eq!(ledger.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(ledger.release_content_digest(), RELEASE_DIGEST);
    assert_eq!(ledger.locale(), "ko-KR");
    assert!(ledger.is_empty());
    assert_eq!(ledger.len(), 0);
}

#[test]
fn active_session_delivery_records_server_ordered_evidence() {
    let mut ledger = ledger();

    let first = ledger
        .deliver(
            SessionState::Active,
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        )
        .unwrap();
    let second = ledger
        .deliver(
            SessionState::Active,
            request(
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
        )
        .unwrap();

    assert_eq!(first.delivery_ref(), "delivery_event_001");
    assert_eq!(first.item_version_ref(), "item_version_001");
    assert_eq!(first.presentation_context_ref(), "presentation_standard_v1");
    assert_eq!(first.selection_evidence_ref(), None);
    assert_eq!(first.sequence(), 1);

    assert_eq!(second.delivery_ref(), "delivery_event_002");
    assert_eq!(second.item_version_ref(), "item_version_002");
    assert_eq!(
        second.selection_evidence_ref(),
        Some("selection_fixed_order_v1")
    );
    assert_eq!(second.sequence(), 2);
    assert_eq!(ledger.events(), [first, second]);
}

#[test]
fn exact_delivery_replay_is_idempotent_but_conflicting_reuse_fails_closed() {
    let mut ledger = ledger();
    let original = request(
        "delivery_event_001",
        "item_version_001",
        "presentation_standard_v1",
        Some("selection_fixed_order_v1"),
    );

    let accepted = ledger.deliver(SessionState::Active, original).unwrap();
    let replayed = ledger.deliver(SessionState::Active, original).unwrap();
    assert_eq!(accepted, replayed);
    assert_eq!(ledger.len(), 1);

    for conflicting in [
        request(
            "delivery_event_001",
            "item_version_002",
            "presentation_standard_v1",
            Some("selection_fixed_order_v1"),
        ),
        request(
            "delivery_event_001",
            "item_version_001",
            "presentation_accessible_v1",
            Some("selection_fixed_order_v1"),
        ),
        request(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            Some("selection_cat_step_001"),
        ),
        request(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        ),
    ] {
        assert_eq!(
            ledger.deliver(SessionState::Active, conflicting),
            Err(ItemDeliveryError::IdempotencyConflict)
        );
    }
}

#[test]
fn same_item_cannot_be_delivered_twice_under_a_new_identity() {
    let mut ledger = ledger();
    ledger
        .deliver(
            SessionState::Active,
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        )
        .unwrap();

    assert_eq!(
        ledger.deliver(
            SessionState::Active,
            request(
                "delivery_event_002",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        ),
        Err(ItemDeliveryError::DuplicateItemDelivery)
    );
}

#[test]
fn item_delivery_requires_an_active_session() {
    for state in [
        SessionState::Created,
        SessionState::Paused,
        SessionState::Completed,
        SessionState::Scoring,
        SessionState::Scored,
        SessionState::Released,
        SessionState::Expired,
        SessionState::Cancelled,
        SessionState::Invalidated,
    ] {
        let mut ledger = ledger();
        assert_eq!(
            ledger.deliver(
                state,
                request(
                    "delivery_event_001",
                    "item_version_001",
                    "presentation_standard_v1",
                    None,
                ),
            ),
            Err(ItemDeliveryError::SessionNotActive(state))
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn ledger_and_request_identity_fail_closed() {
    for invalid in ["", "   ", "12345"] {
        assert_eq!(
            ItemDeliveryLedger::new(invalid, "release_big_five_ko_v1", RELEASE_DIGEST, "ko-KR"),
            Err(ItemDeliveryError::InvalidReference)
        );
        assert_eq!(
            ItemDeliveryLedger::new("session_big_five_001", invalid, RELEASE_DIGEST, "ko-KR"),
            Err(ItemDeliveryError::InvalidReference)
        );
    }

    for digest in [
        "",
        "sha256:0123",
        "md5:0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeF",
    ] {
        assert_eq!(
            ItemDeliveryLedger::new(
                "session_big_five_001",
                "release_big_five_ko_v1",
                digest,
                "ko-KR"
            ),
            Err(ItemDeliveryError::InvalidDigest)
        );
    }

    for locale in ["", "k", "ko_kr", "ko-", "ko-KR!"] {
        assert_eq!(
            ItemDeliveryLedger::new(
                "session_big_five_001",
                "release_big_five_ko_v1",
                RELEASE_DIGEST,
                locale
            ),
            Err(ItemDeliveryError::InvalidLocale)
        );
    }

    let mut ledger = ledger();
    for invalid_request in [
        request("", "item_version_001", "presentation_standard_v1", None),
        request(
            "12345",
            "item_version_001",
            "presentation_standard_v1",
            None,
        ),
        request(
            "delivery_event_001",
            "12345",
            "presentation_standard_v1",
            None,
        ),
        request("delivery_event_001", "item_version_001", "12345", None),
        request(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            Some("12345"),
        ),
    ] {
        assert_eq!(
            ledger.deliver(SessionState::Active, invalid_request),
            Err(ItemDeliveryError::InvalidReference)
        );
    }
}

#[test]
fn cloned_delivery_evidence_preserves_audit_identity() {
    let mut ledger = ledger();
    let event = ledger
        .deliver(
            SessionState::Active,
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
        )
        .unwrap();

    let cloned_event = event.clone();
    let cloned_ledger = ledger.clone();
    assert_eq!(cloned_event, event);
    assert_eq!(cloned_ledger, ledger);
    assert!(format!("{cloned_event:?}").contains("delivery_event_001"));
}

#[test]
fn delivery_errors_have_stable_safe_text() {
    let cases = [
        (
            ItemDeliveryError::InvalidReference,
            "item delivery references must be opaque non-numeric values",
        ),
        (
            ItemDeliveryError::InvalidDigest,
            "item delivery release digest must be canonical sha256",
        ),
        (
            ItemDeliveryError::InvalidLocale,
            "item delivery locale must be a valid BCP 47-style tag",
        ),
        (
            ItemDeliveryError::SessionNotActive(SessionState::Paused),
            "session Paused cannot deliver assessment items",
        ),
        (
            ItemDeliveryError::IdempotencyConflict,
            "delivery reference was already used for different evidence",
        ),
        (
            ItemDeliveryError::DuplicateItemDelivery,
            "item version was already delivered in this session",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
