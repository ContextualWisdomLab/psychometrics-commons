//! Real `PostgreSQL` contract for the hosted dual-proof account-link write path.
//!
//! A buyer who proves control of both the anonymous session and a Keyverse
//! account must keep that link after process restart. After restore, the write
//! command must refuse new links until inspect is clean so a stale unique
//! enforcer cannot tell the next buyer the ended subject is still taken.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::account_link::AuthenticatedAccountControl;
use psychometrics_commons_runtime::account_link_write::{
    persist_authorized_account_link, persist_authorized_account_unlink,
    recover_participant_for_authenticated_account, AccountLinkWriteError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::{
    apply_participant_identity_link_migration, persist_participant_identity_history,
    reconcile_identity_link_current_projections, IdentityLinkPersistenceDisposition,
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

fn relinked_participant() -> ParticipantRecord {
    let mut participant = anonymous_participant();
    participant
        .link_account(
            "link_event_identity_write",
            "keyverse_issuer_write",
            "keyverse_subject_write",
            "anonymous_proof_write",
            "authenticated_proof_write",
            10_100,
        )
        .unwrap();
    participant
        .record_link_end(
            "link_end_event_identity_write",
            "unlink_evidence_identity_write",
            10_200,
        )
        .unwrap();
    participant
        .link_account(
            "link_event_identity_write_gamma",
            "keyverse_issuer_write_gamma",
            "keyverse_subject_write_gamma",
            "anonymous_proof_write_gamma",
            "authenticated_proof_write_gamma",
            10_300,
        )
        .unwrap();
    participant
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
}

#[test]
fn restore_drift_refuses_dual_proof_write_until_operator_reconciles() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    persist_participant_identity_history(&mut transaction, &relinked_participant()).unwrap();
    transaction.commit().unwrap();
    client
        .batch_execute("DELETE FROM account_link_write_test.current_participant_identity_link;")
        .unwrap();
    client
        .execute(
            "INSERT INTO account_link_write_test.current_participant_identity_link (\
                 participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &"participant_identity_write",
                &"link_event_identity_write",
                &"tenant_identity_write",
                &"keyverse_issuer_write",
                &"keyverse_subject_write",
            ],
        )
        .unwrap();

    let mut rebound = ParticipantRecord::new_anonymous(
        "participant_identity_write_epsilon",
        "tenant_identity_write",
        10_000,
    )
    .unwrap();
    let rebound_anonymous = AnonymousSessionContext::new(
        "tenant_identity_write",
        "participant_identity_write_epsilon",
        "session_identity_write_epsilon",
        "anonymous_proof_write_epsilon",
        11_000,
    )
    .unwrap();
    let rebound_authenticated = AuthenticatedAccountControl::new(
        "tenant_identity_write",
        "keyverse_issuer_write",
        "keyverse_subject_write",
        "authenticated_proof_write_epsilon",
        11_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let drifted = persist_authorized_account_link(
        &mut transaction,
        &mut rebound,
        &rebound_anonymous,
        &rebound_authenticated,
        "link_event_identity_write_epsilon",
        10_500,
    )
    .expect_err("a stale unique enforcer must block the hosted write until restore reconcile");
    transaction.rollback().unwrap();
    assert!(matches!(
        drifted,
        AccountLinkWriteError::CurrentProjectionDrift
    ));
    assert!(
        rebound.linked_subject_ref().is_none(),
        "inspect must refuse before dual-proof authorization mutates the in-memory participant"
    );

    let mut transaction = client.transaction().unwrap();
    reconcile_identity_link_current_projections(&mut transaction).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let inserted = persist_authorized_account_link(
        &mut transaction,
        &mut rebound,
        &rebound_anonymous,
        &rebound_authenticated,
        "link_event_identity_write_epsilon",
        10_500,
    )
    .expect("after restore reconcile the later participant may bind the ended subject");
    transaction.commit().unwrap();
    assert_eq!(inserted, IdentityLinkPersistenceDisposition::Inserted);

    let mut transaction = client.transaction().unwrap();
    let recovered = recover_participant_for_authenticated_account(
        &mut transaction,
        &rebound_authenticated,
        10_600,
    )
    .unwrap()
    .expect("the later participant must recover the ended subject after inspect and reconcile");
    transaction.commit().unwrap();
    assert_eq!(
        recovered.participant_ref(),
        "participant_identity_write_epsilon"
    );
    assert_eq!(
        recovered.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
}

#[test]
fn authorized_unlink_clears_recovery_and_allows_the_same_account_to_relink() {
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

    let mut transaction = client.transaction().unwrap();
    let unlinked = persist_authorized_account_unlink(
        &mut transaction,
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect("a still-valid account proof must persist the unlink");
    transaction.commit().unwrap();
    assert_eq!(unlinked, IdentityLinkPersistenceDisposition::Inserted);
    assert!(participant.linked_subject_ref().is_none());

    let mut replay = anonymous_participant();
    replay
        .link_account(
            "link_event_identity_write",
            "keyverse_issuer_write",
            "keyverse_subject_write",
            "anonymous_proof_write",
            "authenticated_proof_write",
            10_400,
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let replayed = persist_authorized_account_unlink(
        &mut transaction,
        &mut replay,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect("exact unlink replay must stay idempotent");
    transaction.commit().unwrap();
    assert_eq!(replayed, IdentityLinkPersistenceDisposition::Duplicate);

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
        "an unlinked account must not recover the previous participant"
    );

    let mut transaction = client.transaction().unwrap();
    let relinked = persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write_relink",
        10_700,
    )
    .expect("the same participant may relink the freed subject");
    transaction.commit().unwrap();
    assert_eq!(relinked, IdentityLinkPersistenceDisposition::Inserted);

    let mut transaction = client.transaction().unwrap();
    let recovered = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_800,
    )
    .unwrap()
    .expect("relink must restore returning-account recovery");
    transaction.commit().unwrap();
    assert_eq!(recovered.participant_ref(), "participant_identity_write");
    assert_eq!(
        recovered.linked_subject_ref(),
        Some("keyverse_subject_write")
    );
    assert_eq!(
        recovered.link_event_ref(),
        Some("link_event_identity_write_relink")
    );
}

#[test]
fn restore_drift_still_lets_the_current_account_unlink() {
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
    client
        .batch_execute("DELETE FROM account_link_write_test.current_participant_identity_link;")
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let unlinked = persist_authorized_account_unlink(
        &mut transaction,
        &mut participant,
        &authenticated_control(),
        "link_end_event_identity_write",
        10_500,
    )
    .expect(
        "a returning account must unlink from history while restore inspect still reports drift",
    );
    transaction.commit().unwrap();
    assert_eq!(unlinked, IdentityLinkPersistenceDisposition::Inserted);
    assert!(participant.linked_subject_ref().is_none());

    let mut transaction = client.transaction().unwrap();
    let missing = recover_participant_for_authenticated_account(
        &mut transaction,
        &authenticated_control(),
        10_600,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(missing.is_none());
}

#[test]
fn write_command_rejects_serializable_isolation() {
    let _guard = write_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    let error = persist_authorized_account_link(
        &mut transaction,
        &mut participant,
        &anonymous_control(),
        &authenticated_control(),
        "link_event_identity_write",
        10_400,
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AccountLinkWriteError::Persistence(IdentityLinkPersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(participant.linked_subject_ref().is_none());
}
