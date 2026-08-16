//! Dual-proof account-link write and recover commands fail closed before persist.

use psychometrics_commons_runtime::account_link::{
    AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use psychometrics_commons_runtime::account_link_write::{
    require_recoverable_account, AccountLinkWriteError,
};
use psychometrics_commons_runtime::postgres_participant_identity_link::IdentityLinkPersistenceError;
use std::error::Error;

fn authenticated_control() -> AuthenticatedAccountControl {
    AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap()
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

    let drift = AccountLinkWriteError::CurrentProjectionDrift;
    assert_eq!(
        drift.to_string(),
        "operators must run restore reconcile before accepting new account-link writes"
    );
    assert!(drift.source().is_none());

    let from_authorization: AccountLinkWriteError =
        AccountLinkAuthorizationError::InvalidTimestamp.into();
    assert!(matches!(
        from_authorization,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));
    let from_persistence: AccountLinkWriteError =
        IdentityLinkPersistenceError::CorruptHistory.into();
    assert!(matches!(
        from_persistence,
        AccountLinkWriteError::Persistence(IdentityLinkPersistenceError::CorruptHistory)
    ));
}

#[test]
fn expired_authenticated_proof_is_not_recoverable() {
    let error = require_recoverable_account(&authenticated_control(), 10_500)
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
    let error = require_recoverable_account(&authenticated_control(), 0)
        .expect_err("unknown recover time must not look up a participant");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));
}

#[test]
fn current_authenticated_proof_is_recoverable() {
    require_recoverable_account(&authenticated_control(), 10_400)
        .expect("a still-valid account proof may recover a participant");
}
