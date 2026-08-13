//! Contract tests for first-class anonymous assessment authorization context.

use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;

const TENANT_REF: &str = "tnt_6a34f831263448cbbfe4621056774d4f";
const PARTICIPANT_REF: &str = "ptc_4e923e7dd9d44af7b38281a224546b38";
const SESSION_REF: &str = "ses_f437dbb76bf94f6e83fa89227ac9da77";
const EVIDENCE_REF: &str = "evd_b18845e9af6a4bb782cb3b376dfd5f85";

fn context() -> AnonymousSessionContext {
    AnonymousSessionContext::new(TENANT_REF, PARTICIPANT_REF, SESSION_REF, EVIDENCE_REF, 2_000)
        .unwrap()
}

#[test]
fn anonymous_session_context_is_a_product_authorization_primitive() {
    let context = context();
    assert_eq!(context.tenant_ref(), TENANT_REF);
    assert_eq!(context.participant_ref(), PARTICIPANT_REF);
    assert_eq!(context.session_ref(), SESSION_REF);
    assert_eq!(context.authorization_evidence_ref(), EVIDENCE_REF);
    assert_eq!(context.valid_until_unix_ms(), 2_000);
    assert!(!context.is_valid_at(0));
    assert!(context.is_valid_at(1_999));
    assert!(!context.is_valid_at(2_000));
}

#[test]
fn anonymous_session_context_matches_only_its_exact_resource_binding() {
    let context = context();

    assert!(context.matches_binding(TENANT_REF, PARTICIPANT_REF, SESSION_REF));
    assert!(!context.matches_binding(
        "tnt_cac42379ce21415d9ac0a6d50f1aeafd",
        PARTICIPANT_REF,
        SESSION_REF,
    ));
    assert!(!context.matches_binding(
        TENANT_REF,
        "ptc_53a34f35f31b49379504b9a655fc2d98",
        SESSION_REF,
    ));
    assert!(!context.matches_binding(
        TENANT_REF,
        PARTICIPANT_REF,
        "ses_67924db89dc7457183172116f9fb8f21",
    ));
    assert!(!context.matches_binding("", PARTICIPANT_REF, SESSION_REF));
}

#[test]
fn anonymous_session_binding_rejects_noncanonical_reference_spellings() {
    let context = context();

    let padded_tenant = format!(" {TENANT_REF}");
    let padded_participant = format!("{PARTICIPANT_REF} ");
    let padded_session = format!(" {SESSION_REF} ");
    assert!(!context.matches_binding(&padded_tenant, PARTICIPANT_REF, SESSION_REF));
    assert!(!context.matches_binding(TENANT_REF, &padded_participant, SESSION_REF));
    assert!(!context.matches_binding(TENANT_REF, PARTICIPANT_REF, &padded_session));
    assert!(!context.is_valid_for_binding_at(
        &format!("{TENANT_REF} "),
        PARTICIPANT_REF,
        SESSION_REF,
        1_000,
    ));
}

#[test]
fn anonymous_session_context_combines_exact_binding_with_expiry_fail_closed() {
    let context = context();

    assert!(context.is_valid_for_binding_at(TENANT_REF, PARTICIPANT_REF, SESSION_REF, 1_999));
    assert!(!context.is_valid_for_binding_at(TENANT_REF, PARTICIPANT_REF, SESSION_REF, 2_000));
    assert!(!context.is_valid_for_binding_at(
        TENANT_REF,
        PARTICIPANT_REF,
        "ses_67924db89dc7457183172116f9fb8f21",
        1_000,
    ));
    assert!(!context.is_valid_for_binding_at(TENANT_REF, PARTICIPANT_REF, SESSION_REF, 0));
}
