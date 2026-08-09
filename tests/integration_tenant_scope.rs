//! Regression tests for tenant/resource-bound integration evidence.

use psychometrics_commons_runtime::integration::{
    InboxDisposition, IntegrationError, IntegrationEvent, IntegrationInbox,
};

const DIGEST_A: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DIGEST_B: &str = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

fn event(tenant_ref: &str, subject_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        "event_shared",
        "assessment.scoring.requested",
        "v1",
        "psychometrics_commons",
        tenant_ref,
        subject_ref,
        10_000,
        "correlation_alpha",
        None,
        digest,
    )
    .unwrap()
}

#[test]
fn event_and_inbox_preserve_tenant_and_subject_binding() {
    let source_event = event("tenant_alpha", "session_alpha", DIGEST_A);
    assert_eq!(source_event.tenant_ref(), "tenant_alpha");
    assert_eq!(source_event.subject_ref(), "session_alpha");

    let mut inbox = IntegrationInbox::new();
    assert_eq!(
        inbox
            .accept_event("scoring_worker", &source_event, 20_000)
            .unwrap(),
        InboxDisposition::Accepted
    );

    let receipt = &inbox.receipts()[0];
    assert_eq!(receipt.tenant_ref(), "tenant_alpha");
    assert_eq!(receipt.subject_ref(), "session_alpha");
    assert_eq!(receipt.source_ref(), "psychometrics_commons");
    assert_eq!(receipt.source_event_ref(), "event_shared");
}

#[test]
fn identical_source_event_refs_are_independent_across_tenants() {
    let mut inbox = IntegrationInbox::new();
    assert_eq!(
        inbox
            .accept_event(
                "scoring_worker",
                &event("tenant_alpha", "session_alpha", DIGEST_A),
                20_000,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(
        inbox
            .accept_event(
                "scoring_worker",
                &event("tenant_beta", "session_beta", DIGEST_B),
                20_100,
            )
            .unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(inbox.len(), 2);
}

#[test]
fn same_tenant_source_event_identity_cannot_change_subject_or_digest() {
    let mut inbox = IntegrationInbox::new();
    let original = event("tenant_alpha", "session_alpha", DIGEST_A);
    assert_eq!(
        inbox.accept_event("scoring_worker", &original, 20_000).unwrap(),
        InboxDisposition::Accepted
    );

    let changed_subject = event("tenant_alpha", "session_other", DIGEST_A);
    assert_eq!(
        inbox.accept_event("scoring_worker", &changed_subject, 20_100),
        Err(IntegrationError::ConflictingReplay)
    );

    let changed_digest = event("tenant_alpha", "session_alpha", DIGEST_B);
    assert_eq!(
        inbox.accept_event("scoring_worker", &changed_digest, 20_100),
        Err(IntegrationError::ConflictingReplay)
    );
}

#[test]
fn invalid_tenant_reference_is_rejected_at_event_boundary() {
    assert_eq!(
        IntegrationEvent::new(
            "event_alpha",
            "assessment.scoring.requested",
            "v1",
            "psychometrics_commons",
            "12345",
            "session_alpha",
            10_000,
            "correlation_alpha",
            None,
            DIGEST_A,
        ),
        Err(IntegrationError::InvalidReference)
    );
}
