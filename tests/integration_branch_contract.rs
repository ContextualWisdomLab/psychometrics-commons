//! Branch-completeness regressions for integration delivery semantics.

use psychometrics_commons_runtime::integration::{
    DeliveryOutcome, InboxDisposition, IntegrationError, IntegrationEvent, IntegrationInbox,
    OutboxEntry, OutboxState,
};

const DIGEST_A: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn event_with(event_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        1_000,
        "correlation_alpha",
        None,
        digest,
    )
    .unwrap()
}

fn event() -> IntegrationEvent {
    event_with("event_alpha", DIGEST_A)
}

#[test]
fn delivered_outbox_rejects_new_attempt_but_keeps_exact_replay_idempotent() {
    let mut outbox = OutboxEntry::new(event(), 3).unwrap();
    outbox
        .record_attempt("attempt_one", DeliveryOutcome::Delivered, 1_100, None)
        .unwrap();

    assert_eq!(
        outbox.record_attempt(
            "attempt_two",
            DeliveryOutcome::RetryableFailure,
            1_200,
            Some("transport_failure")
        ),
        Err(IntegrationError::TerminalOutboxState)
    );
    assert_eq!(
        outbox
            .record_attempt("attempt_one", DeliveryOutcome::Delivered, 1_100, None)
            .unwrap(),
        OutboxState::Delivered
    );
    assert_eq!(outbox.attempt_count(), 1);
}

#[test]
fn replay_conflict_checks_outcome_time_and_cause_independently() {
    let mut outbox = OutboxEntry::new(event(), 3).unwrap();
    outbox
        .record_attempt(
            "attempt_one",
            DeliveryOutcome::RetryableFailure,
            1_100,
            Some("transport_failure"),
        )
        .unwrap();

    assert_eq!(
        outbox.record_attempt(
            "attempt_one",
            DeliveryOutcome::PermanentFailure,
            1_100,
            Some("transport_failure")
        ),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        outbox.record_attempt(
            "attempt_one",
            DeliveryOutcome::RetryableFailure,
            1_101,
            Some("transport_failure")
        ),
        Err(IntegrationError::ConflictingReplay)
    );
    assert_eq!(
        outbox.record_attempt(
            "attempt_one",
            DeliveryOutcome::RetryableFailure,
            1_100,
            Some("different_failure")
        ),
        Err(IntegrationError::ConflictingReplay)
    );
}

#[test]
fn inbox_identity_comparison_covers_same_consumer_source_and_tenant_with_distinct_events() {
    let mut inbox = IntegrationInbox::new();
    assert_eq!(
        inbox
            .accept_event(
                "consumer_alpha",
                &event_with("event_alpha", DIGEST_A),
                2_000,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(
        inbox
            .accept_event("consumer_alpha", &event_with("event_beta", DIGEST_B), 2_100,)
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(inbox.len(), 2);
}
