//! Real `PostgreSQL` contract for the hosted dual-proof account-link write path.
//!
//! A buyer who proves control of both the anonymous session and a Keyverse
//! account must keep that link after process restart, and a later login with
//! the same valid account proof must recover the same product-owned participant.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::account_link::AuthenticatedAccountControl;
use psychometrics_commons_runtime::account_link_write::{
    persist_authorized_account_link, recover_participant_for_authenticated_account,
    AccountLinkWriteError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::{
    apply_participant_identity_link_migration, IdentityLinkPersistenceDisposition,
    IdentityLinkPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static ACCOUNT_LINK_WRITE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn write_test_guard() -> MutexGuard<'static, ()> {
    ACCOUNT_LINK_WRITE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS account_link_write_test;\
             SET search_path TO account_link_write_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS account_link_write_test.current_participant_identity_link;\
             DROP TABLE IF EXISTS account_link_write_test.participant_identity_link_end;\
             DROP TABLE IF EXISTS account_link_write_test.participant_identity_link;\
             DROP TABLE IF EXISTS account_link_write_test.assessment_participant;",
        )
        .unwrap();
}

fn anonymous_participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous(
        "participant_identity_write",
        "tenant_identity_write",
        10_000,
    )
    .unwrap()
}

fn anonymous_control() -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        "tenant_identity_write",
        "participant_identity_write",
        "session_identity_write",
        "anonymous_proof_write",
        11_000,
    )
    .unwrap()
}

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

#[test]
fn dual_proof_link_survives_restart_and_returning_account_recovers_same_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(disposition, IdentityLinkPersistenceDisposition::Inserted);
    assert_eq!(participant.participant_ref(), "participant_identity_write");
    assert_eq!(
        participant.linked_subject_ref(),
        Some("keyverse_subject_write")
    );

    let mut replay = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    let replayed = persist_authorized_account_link(
        &mut transaction,
        &mut replay,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(replayed, IdentityLinkPersistenceDisposition::Duplicate);

    let mut transaction = client.transaction().unwrap();
    let recovered = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_600,
    )
    .unwrap()
    .expect("a returning account must recover the same participant");
    transaction.commit().unwrap();
    assert_eq!(recovered.participant_ref(), "participant_identity_write");
    assert_eq!(recovered.tenant_ref(), "tenant_identity_write");
    assert_eq!(
        recovered.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
    assert_eq!(
        recovered.link_event_ref(),
        Some("link_event_identity_write")
    );
}

#[test]
fn expired_or_foreign_proof_fails_before_persist_or_recovery() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    let expired_authenticated = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_300,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let expired = persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &expired_authenticated,
        "link_event_identity_write",
        10_400,
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        expired,
        AccountLinkWriteError::Authorization(
            psychometrics_commons_runtime::account_link::AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));
    assert!(participant.linked_subject_ref().is_none());

    let mut transaction = client.transaction().unwrap();
    let missing = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_600,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(
        missing.is_none(),
        "an unused valid account proof must not invent a participant"
    );
}

#[test]
fn subject_already_bound_stays_on_the_first_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut first = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    persist_authorized_account_link(
        &mut transaction,
        &mut first,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap();
    transaction.commit().unwrap();

    let mut second = ParticipantRecord::new_anonymous(
        "participant_identity_write_beta",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    let second_anonymous = AnonymousSessionContext::new(
        "tenant_identity_write",
        "participant_identity_write_beta",
        "session_identity_write_beta",
        "anonymous_proof_write_beta",
        11_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_authorized_account_link(
        &mut transaction,
        &mut second,
        &second_anonymous,
        &authenticated_control(),
        "link_event_identity_write_beta",
        10_450,
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AccountLinkWriteError::Persistence(IdentityLinkPersistenceError::SubjectAlreadyBound)
    ));
    assert_eq!(
        second.linked_subject_ref(),
        Some("keyverse_subject_write"),
        "drop the in-memory second participant after persist fails; do not retry the dirty record"
    );

    let mut transaction = client.transaction().unwrap();
    let recovered = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_600,
    )
    .unwrap()
    .expect("the first participant remains the current binding after rollback");
    transaction.commit().unwrap();
    assert_eq!(recovered.participant_ref(), "participant_identity_write");
}

#[test]
fn expired_proof_after_successful_link_cannot_recover_the_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap();
    transaction.commit().unwrap();

    let expired = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write",
        10_500,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = recover_participant_for_authenticated_account(&mut transaction, &expired, 10_500)
        .expect_err("an expired account proof must not look up a linked participant");
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AccountLinkWriteError::Authorization(
            psychometrics_commons_runtime::account_link::AccountLinkAuthorizationError::AuthenticatedProofExpired
        )
    ));
}

#[test]
fn other_tenant_or_ended_subject_cannot_recover_the_linked_participant() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap();
    transaction.commit().unwrap();

    let foreign_tenant = AuthenticatedAccountControl::new(
        "tenant_identity_foreign",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_foreign",
        11_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let missing = recover_participant_for_authenticated_account(
        &mut transaction,
        &foreign_tenant,
        10_600,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(
        missing.is_none(),
        "a valid other-tenant proof must not recover this tenant's participant"
    );

    participant
        .record_link_end(
            "link_end_event_identity_write",
            "unlink_evidence_identity_write",
            10_500,
        )
        .unwrap();
    participant
        .link_account(
            "link_event_identity_rebound",
            "keyverse_issuer_write",
            "keyverse_subject_rebound",
            "anonymous_proof_write",
            "authenticated_proof_rebound",
            10_550,
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    psychometrics_commons_runtime::postgres_participant_identity_link::persist_participant_identity_history(
        &mut transaction,
        &participant,
    )
    .unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let ended = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_600,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(
        ended.is_none(),
        "a still-valid proof for an ended subject must not recover the rebound participant"
    );

    let rebound = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_rebound",
        "authenticated_proof_rebound",
        11_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let recovered = recover_participant_for_authenticated_account(&mut transaction, &rebound, 10_600)
        .unwrap()
        .expect("the current rebound subject must recover the same participant");
    transaction.commit().unwrap();
    assert_eq!(recovered.participant_ref(), "participant_identity_write");
    assert_eq!(
        recovered.linked_subject_ref(),
        Some("keyverse_subject_rebound")
    );
}
