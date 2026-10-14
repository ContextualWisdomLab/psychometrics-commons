//! Security regressions for exact authorization references.
//!
//! Authorization compares opaque product identities. Inputs that are invisible aliases,
//! control-bearing spellings, or numeric-like values must fail before tenant/owner/role
//! comparison, while ordinary visible multilingual identifiers remain valid.

use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole, ResourceKind, ResourceScope,
};

#[test]
fn authorization_context_rejects_unsafe_aliases_across_identity_fields() {
    let invalid_references = [
        "tenant\u{200b}_alpha",
        "tenant\u{2060}_alpha",
        "tenant\u{fe0f}_alpha",
        "tenant\u{e0001}_alpha",
        "tenant\u{0085}_alpha",
        "1E5",
        "١٫٥",
        "１２．５",
    ];

    for invalid in invalid_references {
        assert_eq!(
            AuthorizationContext::new(
                invalid,
                "subject_alpha",
                Some("participant_alpha"),
                &[ProductRole::Participant],
            ),
            Err(AuthorizationError::InvalidReference),
            "tenant reference {invalid:?} must fail closed"
        );
        assert_eq!(
            AuthorizationContext::new(
                "tenant_alpha",
                invalid,
                Some("participant_alpha"),
                &[ProductRole::Participant],
            ),
            Err(AuthorizationError::InvalidReference),
            "subject reference {invalid:?} must fail closed"
        );
        assert_eq!(
            AuthorizationContext::new(
                "tenant_alpha",
                "subject_alpha",
                Some(invalid),
                &[ProductRole::Participant],
            ),
            Err(AuthorizationError::InvalidReference),
            "participant reference {invalid:?} must fail closed"
        );
    }
}

#[test]
fn participant_resource_scope_rejects_unsafe_aliases_without_blocking_visible_identity_material() {
    for invalid in [
        "result\u{200b}_alpha",
        "result\u{2060}_alpha",
        "result\u{fe0f}_alpha",
        "result\u{e0001}_alpha",
        "result\u{0085}_alpha",
        "1E5",
        "١٫٥",
        "１２．５",
    ] {
        assert_eq!(
            ResourceScope::participant_owned(
                ResourceKind::Result,
                "tenant_alpha",
                "participant_alpha",
                invalid,
            ),
            Err(AuthorizationError::InvalidReference),
            "resource reference {invalid:?} must fail closed"
        );
    }

    for valid in [
        "result_1E5",
        "result_١٫٥",
        "result１２．５",
        "result_東京_가나다",
    ] {
        assert!(
            ResourceScope::participant_owned(
                ResourceKind::Result,
                "tenant_alpha",
                "participant_alpha",
                valid,
            )
            .is_ok(),
            "visible opaque reference {valid:?} must remain valid"
        );
    }
}
