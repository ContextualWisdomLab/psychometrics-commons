//! Real PostgreSQL regression for account-link write atomicity at the caller boundary.
//!
//! A durable uniqueness failure must not leave the caller-owned participant
//! aggregate linked in memory when the database rejected that link.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::account_link::AuthenticatedAccountControl;
use psychometrics_commons_runtime::account_link_write::{
    persist_authorized_account_link, AccountLinkWriteError,
};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::{
    apply_participant_identity_link_migration, IdentityLinkPersistenceError,
};

const FAILED_PERSIST_STATE_LOCK_KEY: i64 = 0x4143_4354_4C4B_4641;

fn test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .batch_execute("SET lock_timeout TO '60s'")
        .expect("database-lock waits should have a finite CI bound");
    guard
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&FAILED_PERSIST_STATE_LOCK_KEY],
        )
        .expect("account-link failed-persist fixture lock should be acquired");
    guard
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS account_link_failed_persist_test;\
             SET search_path TO account_link_failed_persist_test;\
             DROP TABLE IF EXISTS current_participant_identity_link;\
             DROP TABLE IF EXISTS participant_identity_link_end;\
             DROP TABLE IF EXISTS participant_identity_link;\
             DROP TABLE IF EXISTS assessment_participant;",
        )
        .unwrap();
    client
}

fn account_control() -> AuthenticatedAccountControl {
    AuthenticatedAccountControl::new(
        "tenant_failed_persist",
        "keyverse_issuer_failed_persist",
        "keyverse_subject_failed_persist",
        "authenticated_proof_failed_persist",
        20_000,
    )
    .unwrap()
}

#[test]
fn subject_uniqueness_failure_leaves_caller_participant_unchanged() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut first = ParticipantRecord::new_anonymous(
        "participant_failed_persist_first",
        "tenant_failed_persist",
        10_000,
    )
    .unwrap();
    let first_anonymous = AnonymousSessionContext::new(
        "tenant_failed_persist",
        "participant_failed_persist_first",
        "session_failed_persist_first",
        "anonymous_proof_failed_persist_first",
        20_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_authorized_account_link(
        &mut transaction,
        &mut first,
        &first_anonymous,
        &account_control(),
        "link_event_failed_persist_first",
        15_000,
    )
    .unwrap();
    transaction.commit().unwrap();

    let mut second = ParticipantRecord::new_anonymous(
        "participant_failed_persist_second",
        "tenant_failed_persist",
        10_000,
    )
    .unwrap();
    let second_before = second.clone();
    let second_anonymous = AnonymousSessionContext::new(
        "tenant_failed_persist",
        "participant_failed_persist_second",
        "session_failed_persist_second",
        "anonymous_proof_failed_persist_second",
        20_000,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = persist_authorized_account_link(
        &mut transaction,
        &mut second,
        &second_anonymous,
        &account_control(),
        "link_event_failed_persist_second",
        15_100,
    )
    .expect_err("the already-bound subject must fail closed for the second participant");
    transaction.rollback().unwrap();

    assert!(matches!(
        error,
        AccountLinkWriteError::Persistence(IdentityLinkPersistenceError::SubjectAlreadyBound)
    ));
    assert_eq!(
        second, second_before,
        "a rejected durable account link must not leak speculative linked state into the caller-owned participant"
    );
}
