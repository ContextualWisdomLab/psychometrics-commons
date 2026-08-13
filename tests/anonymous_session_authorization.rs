//! Contract tests for first-class anonymous assessment authorization.

use psychometrics_commons_runtime::authorization::AnonymousSessionContext;

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
}
