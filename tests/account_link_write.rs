//! Dual-proof account-link write and recover commands fail closed before persist.

use psychometrics_commons_runtime::account_link::{
    AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use psychometrics_commons_runtime::account_link_write::{
    accept_recovered_participant_for_authenticated_account, AccountLinkWriteError,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::IdentityLinkPersistenceError;
use std::error::Error;

fn authenticated_control() -> AuthenticatedAccountControl {
    AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        11_000,
    )
    .unwrap()
}

fn linked_participant(subject_ref: &str) -> ParticipantRecord {
    let mut participant = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    participant
        .link_account(
            "link_event_identity_write",
            "keyverse_issuer_write",
            subject_ref,
            "anonymous_proof_write",
            "authenticated_proof_write",
            10_400,
        )
        .unwrap();
    participant
}

#[test]
fn write_errors_keep_operator_safe_messages_and_sources() {
    let authorization = AccountLinkWriteError::Authorization(
        AccountLinkAuthorizationError::AnonymousSessionExpired,
    );
    assert_eq!(
        authorization.to_string(),
        "anonymous-session control proof is not valid at the account-link time"
    );
    assert!(authorization.source().is_some());

    let persistence =
        AccountLinkWriteError::Persistence(IdentityLinkPersistenceError::SubjectAlreadyBound);
    assert_eq!(
        persistence.to_string(),
        "this issuer-scoped subject already has a current participant identity link"
    );
    assert!(persistence.source().is_some());
}

#[test]
fn expired_authenticated_proof_is_not_recoverable() {
    let authenticated = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap();
    let error = psychometrics_commons_runtime::account_link_write::require_recoverable_account(
        &authenticated,
        10_500,
    )
    .expect_err("an expired account proof must not recover a participant");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(
            AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));
}

#[test]
fn unknown_recover_time_fails_closed() {
    let authenticated = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap();
    let error = psychometrics_commons_runtime::account_link_write::require_recoverable_account(
        &authenticated,
        0,
    )
    .expect_err("unknown recover time must not look up a participant");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));
}

#[test]
fn recover_does_not_return_a_participant_rebound_to_another_subject() {
    let rebound = linked_participant("keyverse_subject_rebound");
    let accepted = accept_recovered_participant_for_authenticated_account(
        Some(rebound),
        &authenticated_control(),
    );
    assert!(
        accepted.is_none(),
        "a still-valid proof must not recover a participant now bound to another subject"
    );
}

#[test]
fn recover_keeps_a_participant_whose_current_binding_matches_the_proof() {
    let current = linked_participant("keyverse_subject_write");
    let accepted = accept_recovered_participant_for_authenticated_account(
        Some(current),
        &authenticated_control(),
    )
    .expect("a matching current binding must remain recoverable");
    assert_eq!(accepted.participant_ref(), "participant_identity_write");
    assert_eq!(
        accepted.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
}

#[test]
fn recover_treats_a_missing_or_unlinked_load_as_unused() {
    assert!(
        accept_recovered_participant_for_authenticated_account(None, &authenticated_control())
            .is_none()
    );

    let unlinked = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    assert!(
        accept_recovered_participant_for_authenticated_account(
            Some(unlinked),
            &authenticated_control(),
        )
        .is_none(),
        "an unlinked participant is not currently bound to the proof"
    );
}

#[test]
fn recover_rejects_tenant_or_issuer_mismatch_after_load() {
    let mut foreign_tenant = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_foreign",
        10_000,
    )
    .unwrap();
    foreign_tenant
        .link_account(
            "link_event_identity_write",
            "keyverse_issuer_write",
            "keyverse_subject_write",
            "anonymous_proof_write",
            "authenticated_proof_write",
            10_400,
        )
        .unwrap();
    assert!(
        accept_recovered_participant_for_authenticated_account(
            Some(foreign_tenant),
            &authenticated_control(),
        )
        .is_none()
    );

    let mut foreign_issuer = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    foreign_issuer
        .link_account(
            "link_event_identity_write",
            "keyverse_issuer_foreign",
            "keyverse_subject_write",
            "anonymous_proof_write",
            "authenticated_proof_write",
            10_400,
        )
        .unwrap();
    assert!(
        accept_recovered_participant_for_authenticated_account(
            Some(foreign_issuer),
            &authenticated_control(),
        )
        .is_none()
    );
}
