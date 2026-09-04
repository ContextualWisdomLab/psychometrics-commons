//! Dual-proof account-link write and recover commands fail closed before persist.

use psychometrics_commons_runtime::account_link::{
    AccountLinkAuthorizationError, AuthenticatedAccountControl,
};
use psychometrics_commons_runtime::account_link_write::{
    accept_account_linked_capability, accept_recovered_participant_for_authenticated_account,
    authorize_account_unlink, grant_account_linked_capability, AccountLinkWriteError,
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

    let no_current = AccountLinkWriteError::NoCurrentBinding;
    assert_eq!(
        no_current.to_string(),
        "this authenticated account is not the participant's current identity link"
    );
    assert!(no_current.source().is_none());
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
    assert!(accept_recovered_participant_for_authenticated_account(
        Some(foreign_tenant),
        &authenticated_control(),
    )
    .is_none());

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
    assert!(accept_recovered_participant_for_authenticated_account(
        Some(foreign_issuer),
        &authenticated_control(),
    )
    .is_none());
}

#[test]
fn unlink_ends_the_current_binding_for_a_still_valid_account_proof() {
    let mut participant = linked_participant("keyverse_subject_write");
    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect("a matching current proof must end the current identity link");
    assert!(participant.linked_subject_ref().is_none());
    assert_eq!(participant.link_end_history().len(), 1);
    assert_eq!(
        participant.link_end_history()[0].link_end_event_ref(),
        "link_end_event_identity_write"
    );
}

#[test]
fn unlink_replay_of_the_same_end_event_is_idempotent() {
    let mut participant = linked_participant("keyverse_subject_write");
    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .unwrap();
    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect("exact unlink replay must not append a second end event");
    assert_eq!(participant.link_end_history().len(), 1);
    assert!(participant.linked_subject_ref().is_none());
}

#[test]
fn unlink_rejects_a_proof_for_a_rebound_current_subject() {
    let mut rebound = linked_participant("keyverse_subject_rebound");
    let error = authorize_account_unlink(
        &mut rebound,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect_err("an ended subject's proof must not unlink a rebound current binding");
    assert!(matches!(error, AccountLinkWriteError::NoCurrentBinding));
    assert_eq!(
        rebound.linked_subject_ref(),
        Some("keyverse_subject_rebound")
    );
}

#[test]
fn expired_authenticated_proof_cannot_unlink() {
    let mut participant = linked_participant("keyverse_subject_write");
    let expired = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap();
    let error = authorize_account_unlink(
        &mut participant,
        &expired,
        "link_end_event_identity_write",
        10_500,
    )
    .expect_err("an expired account proof must not end a current identity link");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(
            AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));
    assert_eq!(
        participant.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
}

#[test]
fn unknown_unlink_time_fails_closed() {
    let mut participant = linked_participant("keyverse_subject_write");
    let error = authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        0,
    )
    .expect_err("unknown unlink time must not end a current identity link");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));
    assert_eq!(
        participant.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
}

#[test]
fn unlink_of_an_unlinked_participant_without_exact_replay_fails_closed() {
    let mut unlinked = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    let error = authorize_account_unlink(
        &mut unlinked,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect_err("an unused account must not invent an unlink against an unlinked participant");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::Participant(_))
    ));
}

#[test]
fn grant_binds_account_capability_to_the_current_link_event() {
    let participant = linked_participant("keyverse_subject_write");
    let capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .expect("a still-valid current proof must grant an account-linked capability")
            .expect("a matching current binding must produce a capability");
    assert_eq!(capability.participant_ref(), "participant_identity_write");
    assert_eq!(capability.tenant_ref(), "tenant_identity_write");
    assert_eq!(capability.issuer_ref(), "keyverse_issuer_write");
    assert_eq!(capability.subject_ref(), "keyverse_subject_write");
    assert_eq!(capability.link_event_ref(), "link_event_identity_write");
}

#[test]
fn accept_keeps_a_grant_only_while_the_current_binding_matches() {
    let mut participant = linked_participant("keyverse_subject_write");
    let capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .unwrap()
            .unwrap();
    accept_account_linked_capability(&participant, &capability, &authenticated_control(), 10_550)
        .expect("the same current binding must still accept the granted capability");

    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_600,
    )
    .unwrap();
    let error = accept_account_linked_capability(
        &participant,
        &capability,
        &authenticated_control(),
        10_650,
    )
    .expect_err("unlink must invalidate a previously granted account-linked capability");
    assert!(matches!(error, AccountLinkWriteError::NoCurrentBinding));
}

#[test]
fn rebound_current_binding_rejects_the_ended_subject_capability() {
    let mut participant = linked_participant("keyverse_subject_write");
    let ended_capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .unwrap()
            .unwrap();
    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_550,
    )
    .unwrap();
    participant
        .link_account(
            "link_event_identity_rebound",
            "keyverse_issuer_write",
            "keyverse_subject_rebound",
            "anonymous_proof_rebound",
            "authenticated_proof_rebound",
            10_600,
        )
        .unwrap();

    assert!(
        grant_account_linked_capability(&participant, &authenticated_control(), 10_650)
            .unwrap()
            .is_none(),
        "an ended subject's proof must not grant a capability for a rebound current binding"
    );
    let error = accept_account_linked_capability(
        &participant,
        &ended_capability,
        &authenticated_control(),
        10_650,
    )
    .expect_err("a rebound participant must not accept the ended subject's capability");
    assert!(matches!(error, AccountLinkWriteError::NoCurrentBinding));
}

#[test]
fn same_subject_relink_with_a_new_event_rejects_the_ended_grant() {
    let mut participant = linked_participant("keyverse_subject_write");
    let ended_capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .unwrap()
            .unwrap();
    authorize_account_unlink(
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_550,
    )
    .unwrap();
    participant
        .link_account(
            "link_event_identity_relink",
            "keyverse_issuer_write",
            "keyverse_subject_write",
            "anonymous_proof_relink",
            "authenticated_proof_relink",
            10_700,
        )
        .unwrap();

    let current_capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_750)
            .unwrap()
            .expect("the same subject must grant a capability bound to the new link event");
    assert_eq!(
        current_capability.link_event_ref(),
        "link_event_identity_relink"
    );
    accept_account_linked_capability(
        &participant,
        &current_capability,
        &authenticated_control(),
        10_760,
    )
    .expect("the new current binding must accept the grant issued for that event");

    let error = accept_account_linked_capability(
        &participant,
        &ended_capability,
        &authenticated_control(),
        10_770,
    )
    .expect_err(
        "a same-subject relink must reject the ended grant so subject match cannot hide a missing link-event check",
    );
    assert!(matches!(error, AccountLinkWriteError::NoCurrentBinding));
}

#[test]
fn expired_or_unknown_time_cannot_grant_or_accept_an_account_capability() {
    let participant = linked_participant("keyverse_subject_write");
    let expired = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap();
    let expired_grant = grant_account_linked_capability(&participant, &expired, 10_500)
        .expect_err("an expired proof must not grant an account-linked capability");
    assert!(matches!(
        expired_grant,
        AccountLinkWriteError::Authorization(
            AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));

    let unknown_grant = grant_account_linked_capability(&participant, &authenticated_control(), 0)
        .expect_err("unknown grant time must not issue an account-linked capability");
    assert!(matches!(
        unknown_grant,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));

    let capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .unwrap()
            .unwrap();
    let expired_accept =
        accept_account_linked_capability(&participant, &capability, &expired, 10_500)
            .expect_err("an expired proof must not accept an account-linked capability");
    assert!(matches!(
        expired_accept,
        AccountLinkWriteError::Authorization(
            AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));

    let unknown_accept =
        accept_account_linked_capability(&participant, &capability, &authenticated_control(), 0)
            .expect_err("unknown accept time must not keep an account-linked capability");
    assert!(matches!(
        unknown_accept,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::InvalidTimestamp)
    ));
}

#[test]
fn accept_rejects_a_foreign_tenant_proof_for_an_issued_grant() {
    let participant = linked_participant("keyverse_subject_write");
    let capability =
        grant_account_linked_capability(&participant, &authenticated_control(), 10_500)
            .unwrap()
            .unwrap();
    let foreign = AuthenticatedAccountControl::new(
        "tenant_identity_foreign",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_foreign",
        11_000,
    )
    .unwrap();
    let error = accept_account_linked_capability(&participant, &capability, &foreign, 10_550)
        .expect_err("a foreign-tenant proof must not accept another tenant's account grant");
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(AccountLinkAuthorizationError::CrossTenantDenied)
    ));
}

#[test]
fn grant_returns_none_for_an_unlinked_or_foreign_tenant_binding() {
    let unlinked = ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    assert!(
        grant_account_linked_capability(&unlinked, &authenticated_control(), 10_500)
            .unwrap()
            .is_none(),
        "an unlinked participant must not receive an account-linked capability"
    );

    let participant = linked_participant("keyverse_subject_write");
    let foreign = AuthenticatedAccountControl::new(
        "tenant_identity_foreign",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_foreign",
        11_000,
    )
    .unwrap();
    assert!(
        grant_account_linked_capability(&participant, &foreign, 10_550)
            .unwrap()
            .is_none(),
        "a foreign-tenant proof must not grant another tenant's account capability"
    );
}
