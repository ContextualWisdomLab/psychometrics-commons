//! Regression tests for source- and tenant-scoped integration inbox deduplication.

use psychometrics_commons_runtime::integration::{
    InboxDisposition, IntegrationError, IntegrationEvent, IntegrationInbox,
};

const DIGEST_A: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn event(source_ref: &str, event_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.scoring.requested",
        "v1",
        source_ref,
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        None,
        digest,
    )
    .unwrap()
}

#[test]
fn identical_event_refs_from_different_sources_do_not_collide() {
    let mut inbox = IntegrationInbox::new();

    assert_eq!(
        inbox
            .accept_event(
                "consumer_alpha",
                &event("source_alpha", "event_shared", DIGEST_A),
                20_000,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(
        inbox
            .accept_event(
                "consumer_alpha",
                &event("source_beta", "event_shared", DIGEST_B),
                20_100,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );

    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox.receipts()[0].source_ref(), "source_alpha");
    assert_eq!(inbox.receipts()[1].source_ref(), "source_beta");
}

#[test]
fn same_source_event_identity_with_changed_digest_still_fails_closed() {
    let mut inbox = IntegrationInbox::new();
    assert_eq!(
        inbox
            .accept_event(
                "consumer_alpha",
                &event("source_alpha", "event_shared", DIGEST_A),
                20_000,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );

    assert_eq!(
        inbox.accept_event(
            "consumer_alpha",
            &event("source_alpha", "event_shared", DIGEST_B),
            20_100,
        ),
        Err(IntegrationError::ConflictingReplay)
    );
}
