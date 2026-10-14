//! Contract tests for durable-integration outbox and inbox semantics.

use psychometrics_commons_runtime::integration::{
    DeliveryOutcome, InboxDisposition, IntegrationError, IntegrationEvent, IntegrationInbox,
    OutboxEntry, OutboxState,
};

const DIGEST_A: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn event_with(event_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.scoring.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        Some("command_complete_alpha"),
        digest,
    )
    .unwrap()
}

fn event() -> IntegrationEvent {
    event_with("event_scoring_requested", DIGEST_A)
}

#[test]
fn integration_event_preserves_versioned_audit_metadata() {
    let event = event();
    assert_eq!(event.event_ref(), "event_scoring_requested");
    assert_eq!(event.event_type(), "assessment.scoring.requested");
    assert_eq!(event.schema_version(), "v1");
    assert_eq!(event.source(), "psychometrics_commons");
    assert_eq!(event.tenant_ref(), "tenant_alpha");
    assert_eq!(event.subject_ref(), "session_alpha");
    assert_eq!(event.occurred_at_unix_ms(), 10_000);
    assert_eq!(event.correlation_ref(), "correlation_alpha");
    assert_eq!(event.causation_ref(), Some("command_complete_alpha"));
    assert_eq!(event.payload_digest(), DIGEST_A);
}

#[test]
fn malformed_event_references_fail_closed() {
    let create = |event_ref: &str,
                  event_type: &str,
                  schema_version: &str,
                  source: &str,
                  subject_ref: &str,
                  occurred_at: u64,
                  correlation_ref: &str,
                  causation_ref: Option<&str>,
                  digest: &str| {
        IntegrationEvent::new(
            event_ref,
            event_type,
            schema_version,
            source,
            "tenant_alpha",
            subject_ref,
            occurred_at,
            correlation_ref,
            causation_ref,
            digest,
        )
    };

    for invalid_ref in ["", "   ", "12345"] {
        assert_eq!(
            create(
                invalid_ref,
                "assessment.scoring.requested",
                "v1",
                "psychometrics_commons",
                "session_alpha",
                10_000,
                "correlation_alpha",
                None,
                DIGEST_A,
            ),
            Err(IntegrationError::InvalidReference)
        );
    }

    assert_eq!(
        create(
            "event_alpha",
            "assessment.scoring.requested",
            "v1",
            "psychometrics_commons",
            "session_alpha",
            10_000,
            "correlation_alpha",
            Some("12345"),
            DIGEST_A,
        ),
        Err(IntegrationError::InvalidReference)
    );
}

#[test]
fn malformed_event_metadata_fails_closed() {
    let create = |event_ref: &str,
                  event_type: &str,
                  schema_version: &str,
                  source: &str,
                  subject_ref: &str,
                  occurred_at: u64,
                  correlation_ref: &str,
                  causation_ref: Option<&str>,
                  digest: &str| {
        IntegrationEvent::new(
            event_ref,
            event_type,
            schema_version,
            source,
            "tenant_alpha",
            subject_ref,
            occurred_at,
            correlation_ref,
            causation_ref,
            digest,
        )
    };

    assert_eq!(
        create(
            "event_alpha",
            "",
            "v1",
            "psychometrics_commons",
            "session_alpha",
            10_000,
            "correlation_alpha",
            None,
            DIGEST_A,
        ),
        Err(IntegrationError::InvalidEventType)
    );
    assert_eq!(
        create(
            "event_alpha",
            "assessment.scoring.requested",
            "",
            "psychometrics_commons",
            "session_alpha",
            10_000,
            "correlation_alpha",
            None,
            DIGEST_A,
        ),
        Err(IntegrationError::InvalidSchemaVersion)
    );
    assert_eq!(
        create(
            "event_alpha",
            "assessment.scoring.requested",
            "v1",
            "psychometrics_commons",
            "session_alpha",
            0,
            "correlation_alpha",
            None,
            DIGEST_A,
        ),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        create(
            "event_alpha",
            "assessment.scoring.requested",
            "v1",
            "psychometrics_commons",
            "session_alpha",
            10_000,
            "correlation_alpha",
            None,
            "sha256:bad",
        ),
        Err(IntegrationError::InvalidDigest)
    );
}

#[test]
fn outbox_retries_are_bounded_and_quarantine_poison_delivery() {
    let mut entry = OutboxEntry::new(event(), 2).unwrap();
    assert_eq!(entry.state(), OutboxState::Pending);
    assert_eq!(entry.max_attempts(), 2);
    assert_eq!(entry.attempt_count(), 0);

    assert_eq!(
        entry
            .record_attempt(
                "attempt_one",
                DeliveryOutcome::RetryableFailure,
                10_100,
                Some("provider_timeout")
            )
            .unwrap(),
        OutboxState::Pending
    );
    assert_eq!(entry.attempt_count(), 1);

    assert_eq!(
        entry
            .record_attempt(
                "attempt_two",
                DeliveryOutcome::RetryableFailure,
                10_200,
                Some("provider_timeout")
            )
            .unwrap(),
        OutboxState::Quarantined
    );
    assert_eq!(entry.attempt_count(), 2);
    assert_eq!(entry.attempts()[0].attempt_ref(), "attempt_one");
    assert_eq!(
        entry.attempts()[0].outcome(),
        DeliveryOutcome::RetryableFailure
    );
    assert_eq!(entry.attempts()[0].occurred_at_unix_ms(), 10_100);
    assert_eq!(entry.attempts()[0].cause_code(), Some("provider_timeout"));

    assert_eq!(
        entry.record_attempt("attempt_three", DeliveryOutcome::Delivered, 10_300, None),
        Err(IntegrationError::TerminalOutboxState)
    );
}

#[test]
fn successful_delivery_is_terminal_and_exact_attempt_replay_is_idempotent() {
    let mut entry = OutboxEntry::new(event(), 3).unwrap();
    assert_eq!(entry.event().event_ref(), "event_scoring_requested");

    assert_eq!(
        entry
            .record_attempt("attempt_one", DeliveryOutcome::Delivered, 10_100, None)
            .unwrap(),
        OutboxState::Delivered
    );
    assert_eq!(
        entry
            .record_attempt("attempt_one", DeliveryOutcome::Delivered, 10_100, None)
            .unwrap(),
        OutboxState::Delivered
    );
    assert_eq!(entry.attempt_count(), 1);

    assert_eq!(
        entry.record_attempt(
            "attempt_one",
            DeliveryOutcome::RetryableFailure,
            10_100,
            Some("different")
        ),
        Err(IntegrationError::ConflictingReplay)
    );
}

#[test]
fn permanent_failure_quarantines_immediately() {
    let mut entry = OutboxEntry::new(event(), 5).unwrap();
    assert_eq!(
        entry
            .record_attempt(
                "attempt_permanent",
                DeliveryOutcome::PermanentFailure,
                10_100,
                Some("schema_rejected")
            )
            .unwrap(),
        OutboxState::Quarantined
    );
    assert_eq!(entry.attempt_count(), 1);
}

#[test]
fn invalid_attempt_evidence_and_time_fail_closed() {
    assert_eq!(
        OutboxEntry::new(event(), 0),
        Err(IntegrationError::InvalidAttemptLimit)
    );
    let mut entry = OutboxEntry::new(event(), 2).unwrap();

    assert_eq!(
        entry.record_attempt("12345", DeliveryOutcome::Delivered, 10_100, None),
        Err(IntegrationError::InvalidReference)
    );
    assert_eq!(
        entry.record_attempt("attempt_zero", DeliveryOutcome::Delivered, 0, None),
        Err(IntegrationError::InvalidTimestamp)
    );
    assert_eq!(
        entry.record_attempt(
            "attempt_backward",
            DeliveryOutcome::RetryableFailure,
            9_999,
            Some("timeout")
        ),
        Err(IntegrationError::NonMonotonicTimestamp)
    );
    assert_eq!(
        entry.record_attempt(
            "attempt_invalid_cause",
            DeliveryOutcome::RetryableFailure,
            10_100,
            Some("12345")
        ),
        Err(IntegrationError::InvalidReference)
    );
}

#[test]
fn inbox_deduplicates_per_consumer_source_tenant_and_rejects_evidence_conflicts() {
    let mut inbox = IntegrationInbox::new();
    assert!(inbox.is_empty());
    let source_event = event();

    assert_eq!(
        inbox
            .accept_event("scoring_worker", &source_event, 20_000)
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox
            .accept_event("scoring_worker", &source_event, 20_000)
            .unwrap(),
        InboxDisposition::Duplicate
    );
    assert_eq!(inbox.len(), 1);

    assert_eq!(
        inbox.accept_event(
            "scoring_worker",
            &event_with("event_scoring_requested", DIGEST_B),
            20_000,
        ),
        Err(IntegrationError::ConflictingReplay)
    );

    assert_eq!(
        inbox
            .accept_event("research_worker", &source_event, 20_100)
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox.receipts()[0].consumer_ref(), "scoring_worker");
    assert_eq!(inbox.receipts()[0].source_ref(), "psychometrics_commons");
    assert_eq!(inbox.receipts()[0].tenant_ref(), "tenant_alpha");
    assert_eq!(
        inbox.receipts()[0].source_event_ref(),
        "event_scoring_requested"
    );
    assert_eq!(
        inbox.receipts()[0].event_type(),
        "assessment.scoring.requested"
    );
    assert_eq!(inbox.receipts()[0].schema_version(), "v1");
    assert_eq!(inbox.receipts()[0].subject_ref(), "session_alpha");
    assert_eq!(inbox.receipts()[0].payload_digest(), DIGEST_A);
    assert_eq!(inbox.receipts()[0].received_at_unix_ms(), 20_000);
}

#[test]
fn inbox_rejects_invalid_consumer_identity_and_timestamp() {
    let mut inbox = IntegrationInbox::new();
    let source_event = event();

    assert_eq!(
        inbox.accept_event("12345", &source_event, 20_000),
        Err(IntegrationError::InvalidReference)
    );
    assert_eq!(
        inbox.accept_event("consumer_alpha", &source_event, 0),
        Err(IntegrationError::InvalidTimestamp)
    );
}

#[test]
fn integration_errors_have_stable_safe_messages() {
    let cases = [
        (
            IntegrationError::InvalidReference,
            "integration references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters",
        ),
        (
            IntegrationError::InvalidEventType,
            "integration event type must be non-empty, bounded, and canonical",
        ),
        (
            IntegrationError::InvalidSchemaVersion,
            "integration schema version must be non-empty, bounded, and canonical",
        ),
        (
            IntegrationError::InvalidDigest,
            "integration payload digest must be a canonical sha256 digest",
        ),
        (
            IntegrationError::InvalidTimestamp,
            "integration timestamps must be greater than zero",
        ),
        (
            IntegrationError::NonMonotonicTimestamp,
            "integration event time must not move backwards",
        ),
        (
            IntegrationError::InvalidAttemptLimit,
            "outbox maximum attempts must be greater than zero",
        ),
        (
            IntegrationError::ConflictingReplay,
            "integration idempotency identity was replayed with conflicting evidence",
        ),
        (
            IntegrationError::TerminalOutboxState,
            "terminal outbox entry cannot accept a new delivery attempt",
        ),
        (
            IntegrationError::TerminalConsumptionState,
            "terminal inbox consumption cannot accept a new processing transition",
        ),
        (
            IntegrationError::ConsumptionNotClaimable,
            "inbox consumption can be claimed only from the pending state",
        ),
        (
            IntegrationError::StaleConsumptionFence,
            "inbox consumption fencing token does not match the current claim",
        ),
        (
            IntegrationError::InvalidConsumptionClaimWindow,
            "inbox consumption claim expiry must be later than claim time",
        ),
        (
            IntegrationError::ConsumptionClaimStillActive,
            "inbox consumption processing claim has not expired",
        ),
        (
            IntegrationError::ConsumptionNotProcessing,
            "inbox consumption claim expiry requires the processing state",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
