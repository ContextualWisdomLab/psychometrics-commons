//! Account linking requires independent current proof of anonymous-session and account control.

use psychometrics_commons_runtime::account_link::{
    link_authenticated_account, AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};
use std::error::Error;

fn participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1_000).unwrap()
}

fn anonymous_for(
    tenant_ref: &str,
    participant_ref: &str,
    evidence_ref: &str,
    valid_until_unix_ms: u64,
) -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        tenant_ref,
        participant_ref,
        "session_alpha",
        evidence_ref,
        valid_until_unix_ms,
    )
    .unwrap()
}

fn authenticated_for(
    tenant_ref: &str,
    proof_evidence_ref: &str,
    valid_until_unix_ms: u64,
) -> AuthenticatedAccountControl {
    AuthenticatedAccountControl::new(
        tenant_ref,
        "issuer_keyverse_prod",
        "subject_account_alpha",
        proof_evidence_ref,
        valid_until_unix_ms,
    )
    .unwrap()
}

#[test]
fn current_independent_control_evidence_links_without_rewriting_participant_identity() {
    let mut participant = participant();
    let anonymous = anonymous_for(
        "tenant_alpha",
        "participant_alpha",
        "anonymous_proof_alpha",
        3_000,
    );
    let authenticated = authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000);

    link_authenticated_account(
        &mut participant,
        &anonymous,
        &authenticated,
        "link_event_alpha",
        2_000,
    )
    .unwrap();

    assert_eq!(participant.participant_ref(), "participant_alpha");
    assert_eq!(participant.tenant_ref(), "tenant_alpha");
    assert_eq!(
        participant.linked_issuer_ref(),
        Some("issuer_keyverse_prod")
    );
    assert_eq!(
        participant.linked_subject_ref(),
        Some("subject_account_alpha")
    );
    assert_eq!(
        participant.anonymous_proof_ref(),
        Some("anonymous_proof_alpha")
    );
    assert_eq!(
        participant.authenticated_proof_ref(),
        Some("authenticated_proof_alpha")
    );
    assert_eq!(participant.link_history().len(), 1);

    link_authenticated_account(
        &mut participant,
        &anonymous,
        &authenticated,
        "link_event_alpha",
        2_000,
    )
    .unwrap();
    assert_eq!(participant.link_history().len(), 1);
}

#[test]
fn expired_or_unknown_time_proof_fails_before_identity_mutation() {
    let cases = [
        (
            anonymous_for(
                "tenant_alpha",
                "participant_alpha",
                "anonymous_proof_alpha",
                2_000,
            ),
            authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000),
            2_000,
            AccountLinkAuthorizationError::AnonymousSessionExpired,
        ),
        (
            anonymous_for(
                "tenant_alpha",
                "participant_alpha",
                "anonymous_proof_alpha",
                3_000,
            ),
            authenticated_for("tenant_alpha", "authenticated_proof_alpha", 2_000),
            2_000,
            AccountLinkAuthorizationError::AuthenticatedProofExpired,
        ),
        (
            anonymous_for(
                "tenant_alpha",
                "participant_alpha",
                "anonymous_proof_alpha",
                3_000,
            ),
            authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000),
            0,
            AccountLinkAuthorizationError::InvalidTimestamp,
        ),
    ];

    for (anonymous, authenticated, now, expected) in cases {
        let mut participant = participant();
        assert_eq!(
            link_authenticated_account(
                &mut participant,
                &anonymous,
                &authenticated,
                "link_event_alpha",
                now,
            )
            .unwrap_err(),
            expected
        );
        assert_eq!(participant.linked_subject_ref(), None);
        assert!(participant.link_history().is_empty());
    }
}

#[test]
fn cross_tenant_and_wrong_participant_anonymous_evidence_fail_closed() {
    let cases = [
        (
            anonymous_for(
                "tenant_other",
                "participant_alpha",
                "anonymous_proof_alpha",
                3_000,
            ),
            authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000),
            AccountLinkAuthorizationError::AnonymousBindingMismatch,
        ),
        (
            anonymous_for(
                "tenant_alpha",
                "participant_other",
                "anonymous_proof_alpha",
                3_000,
            ),
            authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000),
            AccountLinkAuthorizationError::AnonymousBindingMismatch,
        ),
        (
            anonymous_for(
                "tenant_alpha",
                "participant_alpha",
                "anonymous_proof_alpha",
                3_000,
            ),
            authenticated_for("tenant_other", "authenticated_proof_alpha", 3_000),
            AccountLinkAuthorizationError::CrossTenantDenied,
        ),
    ];

    for (anonymous, authenticated, expected) in cases {
        let mut participant = participant();
        assert_eq!(
            link_authenticated_account(
                &mut participant,
                &anonymous,
                &authenticated,
                "link_event_alpha",
                2_000,
            )
            .unwrap_err(),
            expected
        );
        assert!(participant.link_history().is_empty());
    }
}

#[test]
fn account_control_context_rejects_malformed_server_evidence() {
    for (tenant, issuer, subject, proof, expected) in [
        (
            "123",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "authenticated_proof_alpha",
            AccountLinkAuthorizationError::InvalidReference,
        ),
        (
            "tenant_alpha",
            "123",
            "subject_account_alpha",
            "authenticated_proof_alpha",
            AccountLinkAuthorizationError::InvalidReference,
        ),
        (
            "tenant_alpha",
            "issuer_keyverse_prod",
            "123",
            "authenticated_proof_alpha",
            AccountLinkAuthorizationError::InvalidReference,
        ),
        (
            "tenant_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "123",
            AccountLinkAuthorizationError::InvalidReference,
        ),
    ] {
        assert_eq!(
            AuthenticatedAccountControl::new(tenant, issuer, subject, proof, 3_000).unwrap_err(),
            expected
        );
    }
    assert_eq!(
        AuthenticatedAccountControl::new(
            "tenant_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "authenticated_proof_alpha",
            0,
        )
        .unwrap_err(),
        AccountLinkAuthorizationError::InvalidValidityBoundary
    );
}

#[test]
fn authenticated_control_getters_and_validity_are_explicit() {
    let control = authenticated_for("tenant_alpha", "authenticated_proof_alpha", 3_000);

    assert_eq!(control.tenant_ref(), "tenant_alpha");
    assert_eq!(control.issuer_ref(), "issuer_keyverse_prod");
    assert_eq!(control.subject_ref(), "subject_account_alpha");
    assert_eq!(control.proof_evidence_ref(), "authenticated_proof_alpha");
    assert_eq!(control.valid_until_unix_ms(), 3_000);
    assert!(!control.is_valid_at(0));
    assert!(control.is_valid_at(2_999));
    assert!(!control.is_valid_at(3_000));
}

#[test]
fn identical_proof_references_are_rejected_by_the_participant_lifecycle() {
    let mut participant = participant();
    let anonymous = anonymous_for(
        "tenant_alpha",
        "participant_alpha",
        "same_proof_evidence",
        3_000,
    );
    let authenticated = authenticated_for("tenant_alpha", "same_proof_evidence", 3_000);

    assert_eq!(
        link_authenticated_account(
            &mut participant,
            &anonymous,
            &authenticated,
            "link_event_alpha",
            2_000,
        )
        .unwrap_err(),
        AccountLinkAuthorizationError::Participant(AccountLinkError::ProofReferenceReuse)
    );
    assert!(participant.link_history().is_empty());
}

#[test]
fn authorization_errors_have_stable_sources() {
    let direct = [
        AccountLinkAuthorizationError::InvalidReference,
        AccountLinkAuthorizationError::InvalidValidityBoundary,
        AccountLinkAuthorizationError::InvalidTimestamp,
        AccountLinkAuthorizationError::AnonymousSessionExpired,
        AccountLinkAuthorizationError::AnonymousBindingMismatch,
        AccountLinkAuthorizationError::AuthenticatedProofExpired,
        AccountLinkAuthorizationError::CrossTenantDenied,
    ];
    for error in direct {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_none());
    }

    let nested = AccountLinkAuthorizationError::Participant(AccountLinkError::AlreadyLinked);
    assert!(!nested.to_string().is_empty());
    assert!(nested.source().is_some());
}
