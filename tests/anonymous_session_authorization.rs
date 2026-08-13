//! Contract tests for first-class anonymous assessment authorization context.

use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;

#[test]
fn anonymous_session_context_is_a_product_authorization_primitive() {
    let context = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "evidence_alpha",
        2_000,
    )
    .unwrap();
    assert_eq!(context.tenant_ref(), "tenant_alpha");
    assert_eq!(context.participant_ref(), "participant_alpha");
    assert_eq!(context.session_ref(), "session_alpha");
    assert_eq!(context.authorization_evidence_ref(), "evidence_alpha");
    assert_eq!(context.valid_until_unix_ms(), 2_000);
    assert!(!context.is_valid_at(0));
    assert!(context.is_valid_at(1_999));
    assert!(!context.is_valid_at(2_000));
}

#[test]
fn anonymous_session_context_matches_only_its_exact_resource_binding() {
    let context = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "evidence_alpha",
        2_000,
    )
    .unwrap();

    assert!(context.matches_binding(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha"
    ));
    assert!(!context.matches_binding(
        "tenant_beta",
        "participant_alpha",
        "session_alpha"
    ));
    assert!(!context.matches_binding(
        "tenant_alpha",
        "participant_other",
        "session_alpha"
    ));
    assert!(!context.matches_binding(
        "tenant_alpha",
        "participant_alpha",
        "session_other"
    ));
    assert!(!context.matches_binding("", "participant_alpha", "session_alpha"));
}
