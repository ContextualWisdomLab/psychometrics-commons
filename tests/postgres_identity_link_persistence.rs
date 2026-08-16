//! Real `PostgreSQL` contract for durable participant identity-link history.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_identity_link::{
    apply_identity_link_migration, persist_participant_identity,
    IdentityLinkPersistenceDisposition, IdentityLinkPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static IDENTITY_LINK_TEST_LOCK: Mutex<()> = Mutex::new(());

fn identity_link_test_guard() -> MutexGuard<'static, ()> {
    IDENTITY_LINK_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS identity_link_persistence_test;\
             SET search_path TO identity_link_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_identity_link_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS identity_link_persistence_test.participant_identity_link_end_event;\
             DROP TABLE IF EXISTS identity_link_persistence_test.participant_identity_link_event;\
             DROP TABLE IF EXISTS identity_link_persistence_test.participant_identity_ledger;",
        )
        .unwrap();
}

fn cleanup_select_failure_objects(client: &mut Client) {
    client
        .batch_execute(
            "DROP TRIGGER IF EXISTS identity_link_redirect_after_insert \
                 ON identity_link_persistence_test.participant_identity_link_event;\
             DROP FUNCTION IF EXISTS identity_link_persistence_test.identity_link_redirect_after_insert();\
             DROP SCHEMA IF EXISTS identity_link_select_failure_sink CASCADE;\
             DROP TRIGGER IF EXISTS identity_link_end_redirect_after_insert \
                 ON identity_link_persistence_test.participant_identity_link_end_event;\
             DROP FUNCTION IF EXISTS identity_link_persistence_test.identity_link_end_redirect_after_insert();\
             DROP SCHEMA IF EXISTS identity_link_end_select_failure_sink CASCADE;\
             DROP TRIGGER IF EXISTS identity_link_ledger_redirect_after_insert \
                 ON identity_link_persistence_test.participant_identity_ledger;\
             DROP FUNCTION IF EXISTS identity_link_persistence_test.identity_link_ledger_redirect_after_insert();\
             DROP SCHEMA IF EXISTS identity_link_ledger_select_failure_sink CASCADE;",
        )
        .unwrap();
}

fn persist_ok(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> IdentityLinkPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_participant_identity(&mut transaction, participant).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> IdentityLinkPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_participant_identity(&mut transaction, participant).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn anonymous(participant_ref: &str) -> ParticipantRecord {
    ParticipantRecord::new_anonymous(participant_ref, "tenant_clinic_seoul", 10_000).unwrap()
}

fn linked(participant_ref: &str) -> ParticipantRecord {
    let mut participant = anonymous(participant_ref);
    participant
        .link_account(
            "link_event_keyverse_alpha",
            "issuer_keyverse",
            "subject_alpha",
            "proof_anonymous_alpha",
            "proof_authenticated_alpha",
            10_500,
        )
        .unwrap();
    participant
}

#[test]
fn anonymous_participant_persist_is_exactly_idempotent() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = anonymous("participant_identity_alpha");
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Duplicate
    );
}

#[test]
fn link_and_end_history_persist_exactly_and_conflicting_link_fails_closed() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = linked("participant_identity_beta");
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let mut rebound = anonymous("participant_identity_beta");
    rebound
        .link_account(
            "link_event_keyverse_alpha",
            "issuer_keyverse",
            "subject_rebound",
            "proof_anonymous_alpha",
            "proof_authenticated_alpha",
            10_500,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &rebound),
        IdentityLinkPersistenceError::ConflictingReplay
    ));

    let mut ended = linked("participant_identity_beta");
    ended
        .record_link_end("link_end_event_alpha", "evidence_unlink_alpha", 11_000)
        .unwrap();
    assert_eq!(
        persist_ok(&mut client, &ended),
        IdentityLinkPersistenceDisposition::Inserted
    );
    let ends: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM participant_identity_link_end_event \
             WHERE participant_ref = $1",
            &[&"participant_identity_beta"],
        )
        .unwrap()
        .get(0);
    assert_eq!(ends, 1);
    assert_eq!(
        persist_ok(&mut client, &ended),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let tenant_rebind =
        ParticipantRecord::new_anonymous("participant_identity_beta", "tenant_other", 10_000)
            .unwrap();
    assert!(matches!(
        persist_err(&mut client, &tenant_rebind),
        IdentityLinkPersistenceError::ConflictingReplay
    ));
}

#[test]
fn identity_link_persistence_requires_read_committed() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = anonymous("participant_serializable");
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_participant_identity(&mut transaction, &participant),
        Err(IdentityLinkPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_identity_ledger_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);

    let participant = anonymous("participant_missing");
    assert!(matches!(
        persist_err(&mut client, &participant),
        IdentityLinkPersistenceError::Database(_)
    ));
}

#[test]
fn isolated_participants_do_not_share_link_history() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked("participant_left"));
    persist_ok(&mut client, &anonymous("participant_right"));
    let left_links: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM participant_identity_link_event \
             WHERE participant_ref = $1",
            &[&"participant_left"],
        )
        .unwrap()
        .get(0);
    let right_links: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM participant_identity_link_event \
             WHERE participant_ref = $1",
            &[&"participant_right"],
        )
        .unwrap()
        .get(0);
    assert_eq!(left_links, 1);
    assert_eq!(right_links, 0);
}

#[test]
fn oversized_link_timestamp_fails_closed_before_insert() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let mut participant = anonymous("participant_overflow");
    participant
        .link_account(
            "link_event_overflow",
            "issuer_keyverse",
            "subject_overflow",
            "proof_anonymous_overflow",
            "proof_authenticated_overflow",
            u64::MAX,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &participant),
        IdentityLinkPersistenceError::InvalidTimestamp
    ));
}

#[test]
fn replay_select_failure_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = linked("participant_hidden_select");
    persist_ok(&mut client, &participant);
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS identity_link_select_failure_sink CASCADE;\
             CREATE SCHEMA identity_link_select_failure_sink;\
             CREATE OR REPLACE FUNCTION identity_link_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'identity_link_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER identity_link_redirect_after_insert \
             AFTER INSERT ON participant_identity_link_event \
             FOR EACH STATEMENT EXECUTE FUNCTION identity_link_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &participant),
        IdentityLinkPersistenceError::Database(_)
    ));
    cleanup_select_failure_objects(&mut client);
}

#[test]
fn missing_link_event_relation_after_header_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();
    persist_ok(&mut client, &anonymous("participant_missing_event"));
    client
        .batch_execute(
            "DROP TABLE participant_identity_link_end_event;\
             DROP TABLE participant_identity_link_event;",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &linked("participant_missing_event")),
        IdentityLinkPersistenceError::Database(_)
    ));
}

#[test]
fn created_at_rebinding_and_conflicting_end_fail_closed() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &anonymous("participant_created_rebind"));
    let rebound = ParticipantRecord::new_anonymous(
        "participant_created_rebind",
        "tenant_clinic_seoul",
        10_001,
    )
    .unwrap();
    assert!(matches!(
        persist_err(&mut client, &rebound),
        IdentityLinkPersistenceError::ConflictingReplay
    ));

    let mut ended = linked("participant_end_conflict");
    ended
        .record_link_end("link_end_event_alpha", "evidence_unlink_alpha", 11_000)
        .unwrap();
    persist_ok(&mut client, &ended);
    client
        .batch_execute("ALTER TABLE participant_identity_link_end_event DISABLE TRIGGER ALL;")
        .unwrap();
    client
        .execute(
            "UPDATE participant_identity_link_end_event \
             SET evidence_ref = 'evidence_other' \
             WHERE participant_ref = $1 AND link_end_event_ref = $2",
            &[&"participant_end_conflict", &"link_end_event_alpha"],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ended),
        IdentityLinkPersistenceError::ConflictingReplay
    ));
}

#[test]
fn missing_end_event_relation_after_link_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();
    persist_ok(&mut client, &linked("participant_missing_end"));
    client
        .batch_execute("DROP TABLE participant_identity_link_end_event;")
        .unwrap();
    let mut ended = linked("participant_missing_end");
    ended
        .record_link_end("link_end_event_alpha", "evidence_unlink_alpha", 11_000)
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ended),
        IdentityLinkPersistenceError::Database(_)
    ));
}

#[test]
fn end_event_replay_select_failure_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let mut ended = linked("participant_hidden_end_select");
    ended
        .record_link_end("link_end_event_alpha", "evidence_unlink_alpha", 11_000)
        .unwrap();
    persist_ok(&mut client, &ended);
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS identity_link_end_select_failure_sink CASCADE;\
             CREATE SCHEMA identity_link_end_select_failure_sink;\
             CREATE OR REPLACE FUNCTION identity_link_end_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'identity_link_end_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER identity_link_end_redirect_after_insert \
             AFTER INSERT ON participant_identity_link_end_event \
             FOR EACH STATEMENT EXECUTE FUNCTION identity_link_end_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &ended),
        IdentityLinkPersistenceError::Database(_)
    ));
    cleanup_select_failure_objects(&mut client);
}

#[test]
fn ledger_replay_select_failure_is_a_database_failure() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = anonymous("participant_hidden_ledger");
    persist_ok(&mut client, &participant);
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS identity_link_ledger_select_failure_sink CASCADE;\
             CREATE SCHEMA identity_link_ledger_select_failure_sink;\
             CREATE OR REPLACE FUNCTION identity_link_ledger_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'identity_link_ledger_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER identity_link_ledger_redirect_after_insert \
             AFTER INSERT ON participant_identity_ledger \
             FOR EACH STATEMENT EXECUTE FUNCTION identity_link_ledger_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &participant),
        IdentityLinkPersistenceError::Database(_)
    ));
    cleanup_select_failure_objects(&mut client);
}

#[test]
fn each_link_and_end_stored_field_mismatch_fails_closed() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_identity_link_migration(&mut client).unwrap();

    let participant = linked("participant_field_mismatch");
    persist_ok(&mut client, &participant);
    client
        .batch_execute("ALTER TABLE participant_identity_link_event DISABLE TRIGGER ALL;")
        .unwrap();
    for sql in [
        "UPDATE participant_identity_link_event SET issuer_ref = 'issuer_other' \
         WHERE participant_ref = 'participant_field_mismatch'",
        "UPDATE participant_identity_link_event SET anonymous_proof_ref = 'proof_anonymous_other' \
         WHERE participant_ref = 'participant_field_mismatch'",
        "UPDATE participant_identity_link_event SET authenticated_proof_ref = 'proof_authenticated_other' \
         WHERE participant_ref = 'participant_field_mismatch'",
        "UPDATE participant_identity_link_event SET linked_at_unix_ms = 99999 \
         WHERE participant_ref = 'participant_field_mismatch'",
    ] {
        client.batch_execute(sql).unwrap();
        assert!(
            matches!(
                persist_err(&mut client, &participant),
                IdentityLinkPersistenceError::ConflictingReplay
            ),
            "expected conflicting replay for {sql}"
        );
        client
            .batch_execute(
                "UPDATE participant_identity_link_event SET \
                     issuer_ref = 'issuer_keyverse', \
                     anonymous_proof_ref = 'proof_anonymous_alpha', \
                     authenticated_proof_ref = 'proof_authenticated_alpha', \
                     linked_at_unix_ms = 10500 \
                 WHERE participant_ref = 'participant_field_mismatch'",
            )
            .unwrap();
    }

    let mut ended = linked("participant_end_field_mismatch");
    ended
        .record_link_end("link_end_event_alpha", "evidence_unlink_alpha", 11_000)
        .unwrap();
    persist_ok(&mut client, &ended);
    client
        .batch_execute("ALTER TABLE participant_identity_link_end_event DISABLE TRIGGER ALL;")
        .unwrap();
    client
        .execute(
            "INSERT INTO participant_identity_link_event (\
                 participant_ref, link_event_ref, issuer_ref, subject_ref, \
                 anonymous_proof_ref, authenticated_proof_ref, linked_at_unix_ms\
             ) VALUES (\
                 'participant_end_field_mismatch', 'link_event_other', 'issuer_keyverse', \
                 'subject_other', 'proof_anonymous_other', 'proof_authenticated_other', 10501\
             )",
            &[],
        )
        .unwrap();
    for sql in [
        "UPDATE participant_identity_link_end_event SET linked_event_ref = 'link_event_other' \
         WHERE participant_ref = 'participant_end_field_mismatch'",
        "UPDATE participant_identity_link_end_event SET ended_at_unix_ms = 99999 \
         WHERE participant_ref = 'participant_end_field_mismatch'",
    ] {
        client.batch_execute(sql).unwrap();
        assert!(
            matches!(
                persist_err(&mut client, &ended),
                IdentityLinkPersistenceError::ConflictingReplay
            ),
            "expected conflicting replay for {sql}"
        );
        client
            .batch_execute(
                "UPDATE participant_identity_link_end_event SET \
                     linked_event_ref = 'link_event_keyverse_alpha', \
                     evidence_ref = 'evidence_unlink_alpha', \
                     ended_at_unix_ms = 11000 \
                 WHERE participant_ref = 'participant_end_field_mismatch'",
            )
            .unwrap();
    }
}
