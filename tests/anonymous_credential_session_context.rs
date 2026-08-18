//! Verify that a current server-side credential is the source of truth for the session context.
//!
//! The presented proof is hashed before it reaches this contract. A successful proof check may
//! create a session context, but later authorization must still consult the current credential so
//! a revocation recorded after context creation takes effect immediately.

use psychometrics_commons_runtime::anonymous_credential::{
    AnonymousCredential, AnonymousCredentialError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use std::error::Error;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn credential() -> AnonymousCredential {
    AnonymousCredential::new(
        "anonymous_credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST_A,
        1_000,
        2_000,
    )
    .unwrap()
}

#[test]
fn current_exact_digest_mints_the_bound_anonymous_session_context() {
    let credential = credential();

    let context = credential
        .session_context(
            DIGEST_A,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            1_500,
        )
        .unwrap();

    assert_eq!(context.tenant_ref(), "tenant_alpha");
    assert_eq!(context.participant_ref(), "participant_alpha");
    assert_eq!(context.session_ref(), "session_alpha");
    assert_eq!(
        context.authorization_evidence_ref(),
        "anonymous_credential_alpha"
    );
    assert_eq!(context.valid_until_unix_ms(), 2_000);
    assert!(context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_500
    ));
    assert!(credential.authorizes_session_context_at(&context, 1_500));
}

#[test]
fn revocation_after_context_issuance_invalidates_later_authorization() {
    let mut credential = credential();
    let context = credential
        .session_context(
            DIGEST_A,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            1_500,
        )
        .unwrap();

    assert!(credential.authorizes_session_context_at(&context, 1_599));
    credential.revoke(1_600).unwrap();

    // The immutable context still carries its issuance-time expiry. It is therefore not the
    // current revocation authority by itself.
    assert!(context.is_valid_for_binding_at(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        1_700
    ));
    assert!(!credential.authorizes_session_context_at(&context, 1_600));
    assert!(!credential.authorizes_session_context_at(&context, 1_700));
}

#[test]
fn current_credential_rejects_contexts_from_other_evidence_or_bindings() {
    let credential = credential();
    let other_evidence = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "anonymous_credential_other",
        2_000,
    )
    .unwrap();
    assert!(!credential.authorizes_session_context_at(&other_evidence, 1_500));

    let rebound_session = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_other",
        "anonymous_credential_alpha",
        2_000,
    )
    .unwrap();
    assert!(!credential.authorizes_session_context_at(&rebound_session, 1_500));

    let stale_context = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        "anonymous_credential_alpha",
        1_400,
    )
    .unwrap();
    assert!(!credential.authorizes_session_context_at(&stale_context, 1_500));
}

#[test]
fn expired_wrong_or_revoked_proofs_cannot_mint_session_authority() {
    let mut credential = credential();

    assert_eq!(
        credential
            .session_context(
                DIGEST_A,
                "tenant_alpha",
                "participant_alpha",
                "session_alpha",
                2_000,
            )
            .unwrap_err(),
        AnonymousCredentialError::Unauthorized
    );
    assert_eq!(
        credential
            .session_context(
                DIGEST_B,
                "tenant_alpha",
                "participant_alpha",
                "session_alpha",
                1_500,
            )
            .unwrap_err(),
        AnonymousCredentialError::Unauthorized
    );
    assert_eq!(
        credential
            .session_context(
                DIGEST_A,
                "tenant_other",
                "participant_alpha",
                "session_alpha",
                1_500,
            )
            .unwrap_err(),
        AnonymousCredentialError::Unauthorized
    );

    credential.revoke(1_400).unwrap();
    assert_eq!(
        credential
            .session_context(
                DIGEST_A,
                "tenant_alpha",
                "participant_alpha",
                "session_alpha",
                1_400,
            )
            .unwrap_err(),
        AnonymousCredentialError::Unauthorized
    );

    let revoked_context = credential
        .session_context(
            DIGEST_A,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            1_399,
        )
        .unwrap();
    assert_eq!(revoked_context.valid_until_unix_ms(), 1_400);
    assert!(!revoked_context.is_valid_at(1_400));

    let mut expired_then_revoked = AnonymousCredential::new(
        "anonymous_credential_alpha",
        "tenant_alpha",
        "participant_alpha",
        "session_alpha",
        DIGEST_A,
        1_000,
        2_000,
    )
    .unwrap();
    expired_then_revoked.revoke(2_000).unwrap();
    let context = expired_then_revoked
        .session_context(
            DIGEST_A,
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            1_999,
        )
        .unwrap();
    assert_eq!(context.valid_until_unix_ms(), 2_000);
}

#[test]
fn unauthorized_error_tells_the_caller_to_present_current_exact_proof() {
    let error = AnonymousCredentialError::Unauthorized;
    assert_eq!(
        error.to_string(),
        "present a current exact digest for this tenant, participant, and session"
    );
    assert!(error.source().is_none());
}
