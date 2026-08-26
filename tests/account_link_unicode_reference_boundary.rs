//! Unicode exact-spelling regressions for authenticated account-control evidence.

use psychometrics_commons_runtime::account_link::{
    AccountLinkAuthorizationError, AuthenticatedAccountControl,
};

#[test]
fn account_control_rejects_unicode_padded_identity_and_proof_aliases() {
    for (tenant_ref, issuer_ref, subject_ref, proof_evidence_ref) in [
        (
            "\u{00a0}tenant_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "authenticated_proof_alpha",
        ),
        (
            "tenant_alpha",
            "issuer_keyverse_prod\u{2003}",
            "subject_account_alpha",
            "authenticated_proof_alpha",
        ),
        (
            "tenant_alpha",
            "issuer_keyverse_prod",
            "\u{202f}subject_account_alpha",
            "authenticated_proof_alpha",
        ),
        (
            "tenant_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "authenticated_proof_alpha\u{3000}",
        ),
    ] {
        assert_eq!(
            AuthenticatedAccountControl::new(
                tenant_ref,
                issuer_ref,
                subject_ref,
                proof_evidence_ref,
                3_000,
            ),
            Err(AccountLinkAuthorizationError::InvalidReference),
        );
    }
}

#[test]
fn account_control_preserves_exact_visible_unicode_identifiers() {
    let control = AuthenticatedAccountControl::new(
        "tenant_서울",
        "issuer_키버스",
        "subject_사용자_alpha",
        "proof_검증_alpha",
        3_000,
    )
    .expect("visible non-numeric Unicode is a valid opaque reference");

    assert_eq!(control.tenant_ref(), "tenant_서울");
    assert_eq!(control.issuer_ref(), "issuer_키버스");
    assert_eq!(control.subject_ref(), "subject_사용자_alpha");
    assert_eq!(control.proof_evidence_ref(), "proof_검증_alpha");
}
