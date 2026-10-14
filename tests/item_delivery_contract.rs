//! Contract tests for session-bound immutable item-delivery evidence.

mod item_delivery_support;

use item_delivery_support::{published_release, session_in_state, RELEASE_DIGEST};
use psychometrics_commons_runtime::item_delivery::{
    ItemDeliveryError, ItemDeliveryLedger, ItemDeliveryRequest,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCreationError, SessionState,
};

fn ledger_for(state: SessionState) -> (AssessmentSession, ItemDeliveryLedger) {
    let release = published_release();
    let session = session_in_state(&release, state);
    let ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    (session, ledger)
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
fn ledger_pins_session_and_exact_release_manifest_evidence() {
    let (session, ledger) = ledger_for(SessionState::Created);

    assert_eq!(ledger.session_ref(), session.session_ref());
    assert_eq!(ledger.instrument_release_ref(), "release_big_five_ko_v1");
    assert_eq!(ledger.release_content_digest(), RELEASE_DIGEST);
    assert_eq!(ledger.locale(), "ko-KR");
    assert_eq!(
        ledger.allowed_item_version_refs(),
        ["item_version_001", "item_version_002"]
    );
    assert!(ledger.is_empty());
    assert_eq!(ledger.len(), 0);
}

#[test]
fn active_session_delivery_records_server_ordered_evidence() {
    let (session, mut ledger) = ledger_for(SessionState::Active);

    let first = ledger
        .deliver(
            &session,
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
            &session,
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
    let (session, mut ledger) = ledger_for(SessionState::Active);
    let original = request(
        "delivery_event_001",
        "item_version_001",
        "presentation_standard_v1",
        Some("selection_fixed_order_v1"),
    );

    let accepted = ledger.deliver(&session, original).unwrap();
    let replayed = ledger.deliver(&session, original).unwrap();
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
            ledger.deliver(&session, conflicting),
            Err(ItemDeliveryError::IdempotencyConflict)
        );
    }
}

#[test]
fn exact_replay_remains_idempotent_after_session_stops_accepting_new_deliveries() {
    let release = published_release();
    let active_session = session_in_state(&release, SessionState::Active);
    let mut ledger = ItemDeliveryLedger::from_session(&active_session, release.manifest()).unwrap();
    let original = request(
        "delivery_event_001",
        "item_version_001",
        "presentation_standard_v1",
        Some("selection_fixed_order_v1"),
    );
    let accepted = ledger.deliver(&active_session, original).unwrap();

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
        let later_session = session_in_state(&release, state);
        assert_eq!(
            ledger.deliver(&later_session, original),
            Ok(accepted.clone())
        );
    }
    assert_eq!(ledger.len(), 1);
}

#[test]
fn item_outside_bound_release_fails_closed_before_duplicate_item_logic() {
    let (session, mut ledger) = ledger_for(SessionState::Active);
    assert_eq!(
        ledger.deliver(
            &session,
            request(
                "delivery_event_outside",
                "item_version_outside_release",
                "presentation_standard_v1",
                None,
            ),
        ),
        Err(ItemDeliveryError::ItemNotInRelease)
    );
    assert!(ledger.is_empty());
}

#[test]
fn same_item_cannot_be_delivered_twice_under_a_new_identity() {
    let (session, mut ledger) = ledger_for(SessionState::Active);
    ledger
        .deliver(
            &session,
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
            &session,
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
fn item_delivery_requires_the_authoritative_session_to_be_active() {
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
        let (session, mut ledger) = ledger_for(state);
        assert_eq!(
            ledger.deliver(
                &session,
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
fn request_identity_fails_closed() {
    let (session, mut ledger) = ledger_for(SessionState::Active);
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
            ledger.deliver(&session, invalid_request),
            Err(ItemDeliveryError::InvalidReference)
        );
    }
}

#[test]
fn a_different_session_cannot_operate_the_ledger() {
    let release = published_release();
    let session = session_in_state(&release, SessionState::Active);
    let mut ledger = ItemDeliveryLedger::from_session(&session, release.manifest()).unwrap();
    let different_session = AssessmentSession::new(
        "session_big_five_002",
        "participant_big_five_001",
        &release,
        "ko-KR",
        21_000,
    )
    .unwrap();

    assert_eq!(
        ledger.deliver(
            &different_session,
            request(
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        ),
        Err(ItemDeliveryError::SessionMismatch)
    );
    assert!(ledger.is_empty());
}

#[test]
fn cloned_delivery_evidence_preserves_audit_identity() {
    let (session, mut ledger) = ledger_for(SessionState::Active);
    let event = ledger
        .deliver(
            &session,
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
            ItemDeliveryError::SessionReleaseMismatch,
            "item delivery manifest does not match assessment session provenance",
        ),
        (
            ItemDeliveryError::SessionMismatch,
            "item delivery ledger does not belong to the supplied assessment session",
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
            ItemDeliveryError::ItemNotInRelease,
            "item version is not part of the bound instrument release manifest",
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

#[test]
fn session_creation_rejects_padded_session_aliases_before_delivery_binding() {
    let release = published_release();

    for padded in [
        " session_big_five_001",
        "session_big_five_001 ",
        "\u{2003}session_big_five_001",
    ] {
        assert_eq!(
            AssessmentSession::new(
                padded,
                "participant_big_five_001",
                &release,
                "ko-KR",
                20_000,
            ),
            Err(SessionCreationError::InvalidReference),
            "a padded session alias must fail closed before any delivery binding {padded:?}",
        );
    }
}
