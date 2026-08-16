//! Real `PostgreSQL` contract for append-only participant identity-link history.
//!
//! A buyer who links an anonymous assessment to a Keyverse account must still
//! see that link after process restart. Historical participant identity must
//! stay stable across link, unlink, and relink.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::{
    apply_participant_identity_link_migration, load_participant_identity_history,
    persist_participant_identity_history, IdentityLinkPersistenceDisposition,
    IdentityLinkPersistenceError,
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
            "DROP TABLE IF EXISTS identity_link_persistence_test.current_participant_identity_link;\
             DROP TABLE IF EXISTS identity_link_persistence_test.participant_identity_link_end;\
             DROP TABLE IF EXISTS identity_link_persistence_test.participant_identity_link;\
             DROP TABLE IF EXISTS identity_link_persistence_test.assessment_participant;",
        )
        .unwrap();
}

fn anonymous_participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous(
        "participant_identity_alpha",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap()
}

fn linked_participant() -> ParticipantRecord {
    let mut participant = anonymous_participant();
    participant
        .link_account(
            "link_event_identity_alpha",
            "keyverse_issuer_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_identity_alpha",
            "authenticated_proof_identity_alpha",
            10_100,
        )
        .unwrap();
    participant
}

fn relinked_participant() -> ParticipantRecord {
    let mut participant = linked_participant();
    participant
        .record_link_end(
            "link_end_event_identity_alpha",
            "unlink_evidence_identity_alpha",
            10_200,
        )
        .unwrap();
    participant
        .link_account(
            "link_event_identity_gamma",
            "keyverse_issuer_gamma",
            "keyverse_subject_gamma",
            "anonymous_proof_identity_gamma",
            "authenticated_proof_identity_gamma",
            10_300,
        )
        .unwrap();
    participant
}

fn persist_ok(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> IdentityLinkPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_participant_identity_history(&mut transaction, participant).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> IdentityLinkPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_participant_identity_history(&mut transaction, participant).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn load_ok(client: &mut Client, participant_ref: &str, tenant_ref: &str) -> ParticipantRecord {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_participant_identity_history(&mut transaction, participant_ref, tenant_ref)
        .unwrap()
        .expect("persisted participant identity history must reload");
    transaction.commit().unwrap();
    loaded
}

#[test]
fn anonymous_participant_survives_restart_without_inventing_a_link() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let participant = anonymous_participant();
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let loaded = load_ok(
        &mut client,
        participant.participant_ref(),
        participant.tenant_ref(),
    );
    assert_eq!(loaded.participant_ref(), "participant_identity_alpha");
    assert_eq!(loaded.tenant_ref(), "tenant_identity_alpha");
    assert_eq!(loaded.created_at_unix_ms(), 10_000);
    assert!(loaded.linked_subject_ref().is_none());
    assert!(loaded.link_history().is_empty());
    assert!(loaded.link_end_history().is_empty());
}

#[test]
fn linked_account_reloads_after_restart_without_rewriting_participant_identity() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let participant = linked_participant();
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let loaded = load_ok(
        &mut client,
        participant.participant_ref(),
        participant.tenant_ref(),
    );
    assert_eq!(loaded.participant_ref(), participant.participant_ref());
    assert_eq!(loaded.linked_issuer_ref(), Some("keyverse_issuer_alpha"));
    assert_eq!(loaded.linked_subject_ref(), Some("keyverse_subject_alpha"));
    assert_eq!(loaded.link_event_ref(), Some("link_event_identity_alpha"));
    assert_eq!(loaded.link_history().len(), 1);
    assert_eq!(
        loaded.link_history()[0].anonymous_proof_ref(),
        "anonymous_proof_identity_alpha"
    );
    assert_eq!(
        loaded.link_history()[0].authenticated_proof_ref(),
        "authenticated_proof_identity_alpha"
    );
    assert_eq!(loaded.link_history()[0].linked_at_unix_ms(), 10_100);
    assert!(loaded.link_end_history().is_empty());
}

#[test]
fn conflicting_link_replay_fails_closed_and_preserves_the_original_evidence() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());

    let mut conflicting = anonymous_participant();
    conflicting
        .link_account(
            "link_event_identity_alpha",
            "keyverse_issuer_beta",
            "keyverse_subject_alpha",
            "anonymous_proof_identity_alpha",
            "authenticated_proof_identity_alpha",
            10_100,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &conflicting),
        IdentityLinkPersistenceError::ConflictingReplay
    ));

    let loaded = load_ok(
        &mut client,
        "participant_identity_alpha",
        "tenant_identity_alpha",
    );
    assert_eq!(loaded.linked_issuer_ref(), Some("keyverse_issuer_alpha"));
}

#[test]
fn unlink_and_relink_reload_as_append_only_history() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let participant = relinked_participant();
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let relinked = load_ok(
        &mut client,
        participant.participant_ref(),
        participant.tenant_ref(),
    );
    assert_eq!(relinked.participant_ref(), "participant_identity_alpha");
    assert_eq!(
        relinked.linked_subject_ref(),
        Some("keyverse_subject_gamma")
    );
    assert_eq!(relinked.linked_issuer_ref(), Some("keyverse_issuer_gamma"));
    assert_eq!(relinked.link_event_ref(), Some("link_event_identity_gamma"));
    assert_eq!(relinked.link_history().len(), 2);
    assert_eq!(relinked.link_end_history().len(), 1);
    assert_eq!(
        relinked.link_end_history()[0].linked_event_ref(),
        "link_event_identity_alpha"
    );
    assert_eq!(
        relinked.link_end_history()[0].evidence_ref(),
        "unlink_evidence_identity_alpha"
    );
}

#[test]
fn unlinked_subject_can_become_current_on_another_participant() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut previous = linked_participant();
    previous
        .record_link_end(
            "link_end_event_identity_alpha",
            "unlink_evidence_identity_alpha",
            10_200,
        )
        .unwrap();
    persist_ok(&mut client, &previous);

    let mut next = ParticipantRecord::new_anonymous(
        "participant_identity_beta",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap();
    next.link_account(
        "link_event_identity_beta",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
        "anonymous_proof_identity_beta",
        "authenticated_proof_identity_beta",
        10_250,
    )
    .unwrap();
    assert_eq!(
        persist_ok(&mut client, &next),
        IdentityLinkPersistenceDisposition::Inserted
    );

    let previous_loaded = load_ok(
        &mut client,
        previous.participant_ref(),
        previous.tenant_ref(),
    );
    let next_loaded = load_ok(&mut client, next.participant_ref(), next.tenant_ref());
    assert!(previous_loaded.linked_subject_ref().is_none());
    assert_eq!(
        next_loaded.linked_subject_ref(),
        Some("keyverse_subject_alpha")
    );
    assert_eq!(
        previous_loaded.participant_ref(),
        "participant_identity_alpha"
    );
    assert_eq!(next_loaded.participant_ref(), "participant_identity_beta");
}

#[test]
fn one_external_subject_cannot_be_current_on_two_participants() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());

    let mut other = ParticipantRecord::new_anonymous(
        "participant_identity_beta",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap();
    other
        .link_account(
            "link_event_identity_beta",
            "keyverse_issuer_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_identity_beta",
            "authenticated_proof_identity_beta",
            10_150,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &other),
        IdentityLinkPersistenceError::SubjectAlreadyBound
    ));
}

#[test]
fn other_tenant_cannot_load_or_rebind_participant_identity() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());

    let mut transaction = client.transaction().unwrap();
    let loaded = load_participant_identity_history(
        &mut transaction,
        "participant_identity_alpha",
        "tenant_identity_other",
    )
    .unwrap();
    transaction.commit().unwrap();
    assert!(loaded.is_none());

    let rebound = ParticipantRecord::new_anonymous(
        "participant_identity_alpha",
        "tenant_identity_other",
        10_000,
    )
    .unwrap();
    assert!(matches!(
        persist_err(&mut client, &rebound),
        IdentityLinkPersistenceError::ConflictingReplay
    ));
}

#[test]
fn serializable_isolation_is_rejected() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    let error = persist_participant_identity_history(&mut transaction, &anonymous_participant())
        .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        IdentityLinkPersistenceError::UnsupportedIsolationLevel
    ));
}
