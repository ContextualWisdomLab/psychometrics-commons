//! Contract tests for first-class anonymous assessment authorization context.

use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;

fn context() -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "evidence_alpha",
        2_000,
    )
    .unwrap()
}

#[test]
fn anonymous_session_context_is_a_product_authorization_primitive() {
    let context = context();
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
    let context = context();

    assert!(context.matches_binding("tenant_alpha", "participant_alpha", "session_alpha"));
    assert!(!context.matches_binding("tenant_beta", "participant_alpha", "session_alpha"));
    assert!(!context.matches_binding("tenant_alpha", "participant_other", "session_alpha"));
    assert!(!context.matches_binding("tenant_alpha", "participant_alpha", "session_other"));
    assert!(!context.matches_binding("", "participant_alpha", "session_alpha"));
}

#[test]
fn anonymous_session_binding_rejects_noncanonical_reference_spellings() {
    let context = context();

    assert!(!context.matches_binding(" tenant_alpha", "participant_alpha", "session_alpha"));
    assert!(!context.matches_binding("tenant_alpha", "participant_alpha ", "session_alpha"));
    assert!(!context.matches_binding("tenant_alpha", "participant_alpha", " session_alpha "));
    assert!(!context.is_valid_for_binding_at(
        "tenant_alpha ",
        "participant_alpha",
        "session_alpha",
        1_000,
    ));
}

#[test]
fn anonymous_session_context_combines_exact_binding_with_expiry_fail_closed() {
    let context = context();

    assert!(context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_999,
    ));
    assert!(!context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        2_000,
    ));
    assert!(!context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_other",
        1_000,
    ));
    assert!(!context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        0,
    ));
}
