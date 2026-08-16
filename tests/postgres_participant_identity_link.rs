//! Real `PostgreSQL` contract for append-only participant identity-link history.
//!
//! A buyer who links an anonymous assessment to a Keyverse account must still
//! see that link after process restart. Historical participant identity must
//! stay stable across link, unlink, and relink.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant_identity_link::{
    apply_participant_identity_link_migration, load_participant_by_current_identity_subject,
    load_participant_identity_history, persist_participant_identity_history,
    reconcile_identity_link_current_projections, IdentityLinkPersistenceDisposition,
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

fn drop_current_projection(client: &mut Client) {
    client
        .batch_execute(
            "DELETE FROM identity_link_persistence_test.current_participant_identity_link;",
        )
        .unwrap();
}

fn current_projection(
    client: &mut Client,
    participant_ref: &str,
) -> Option<(String, String, String)> {
    client
        .query_opt(
            "SELECT identity_link_ref, identity_issuer, identity_subject_ref \
             FROM identity_link_persistence_test.current_participant_identity_link \
             WHERE participant_ref = $1",
            &[&participant_ref],
        )
        .unwrap()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
}

fn anonymous_participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous(
        "participant_identity_alpha",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap()
}

fn anonymous_participant_beta() -> ParticipantRecord {
    ParticipantRecord::new_anonymous("participant_identity_beta", "tenant_identity_alpha", 10_000)
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

fn linked_participant_beta() -> ParticipantRecord {
    let mut participant = anonymous_participant_beta();
    participant
        .link_account(
            "link_event_identity_beta",
            "keyverse_issuer_beta",
            "keyverse_subject_beta",
            "anonymous_proof_identity_beta",
            "authenticated_proof_identity_beta",
            10_150,
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

fn load_by_subject_ok(
    client: &mut Client,
    tenant_ref: &str,
    identity_issuer: &str,
    identity_subject_ref: &str,
) -> Option<ParticipantRecord> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_participant_by_current_identity_subject(
        &mut transaction,
        tenant_ref,
        identity_issuer,
        identity_subject_ref,
    )
    .unwrap();
    transaction.commit().unwrap();
    loaded
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
    let found = load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .expect("the reused subject must resolve to the participant that currently holds it");
    assert_eq!(found.participant_ref(), "participant_identity_beta");
}

#[test]
fn returning_account_finds_the_same_participant_by_current_subject() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());

    let found = load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .expect("a returning Keyverse login must find the stored participant");
    assert_eq!(found.participant_ref(), "participant_identity_alpha");
    assert_eq!(found.linked_subject_ref(), Some("keyverse_subject_alpha"));

    assert!(load_by_subject_ok(
        &mut client,
        "tenant_identity_other",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .is_none());
}

#[test]
fn ended_or_replaced_subject_is_not_findable_until_it_is_current_again() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &relinked_participant());

    assert!(load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .is_none());

    let found = load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_gamma",
        "keyverse_subject_gamma",
    )
    .expect("the current relinked account must resolve to the same participant");
    assert_eq!(found.participant_ref(), "participant_identity_alpha");
    assert_eq!(found.linked_subject_ref(), Some("keyverse_subject_gamma"));
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
fn returning_account_finds_participant_after_current_projection_loss() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    drop_current_projection(&mut client);

    let found = load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .expect("append-only history, not the derived projection, is the source of truth");
    assert_eq!(found.participant_ref(), "participant_identity_alpha");
    assert_eq!(found.linked_subject_ref(), Some("keyverse_subject_alpha"));
}

#[test]
fn lost_current_projection_cannot_rebind_subject_to_another_participant() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    drop_current_projection(&mut client);

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

    let original = load_ok(
        &mut client,
        "participant_identity_alpha",
        "tenant_identity_alpha",
    );
    assert_eq!(original.participant_ref(), "participant_identity_alpha");
    assert_eq!(
        original.linked_subject_ref(),
        Some("keyverse_subject_alpha")
    );
}

#[test]
fn exact_replay_restores_missing_current_projection() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    drop_current_projection(&mut client);
    assert!(current_projection(&mut client, "participant_identity_alpha").is_none());

    assert_eq!(
        persist_ok(&mut client, &linked_participant()),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let restored = current_projection(&mut client, "participant_identity_alpha")
        .expect("exact replay must restore the derived current projection");
    assert_eq!(restored.0, "link_event_identity_alpha");
    assert_eq!(restored.1, "keyverse_issuer_alpha");
    assert_eq!(restored.2, "keyverse_subject_alpha");
}

#[test]
fn exact_replay_of_relink_restores_only_the_current_projection() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &relinked_participant());
    drop_current_projection(&mut client);

    assert_eq!(
        persist_ok(&mut client, &relinked_participant()),
        IdentityLinkPersistenceDisposition::Duplicate
    );

    let restored = current_projection(&mut client, "participant_identity_alpha")
        .expect("relink replay must restore only the current account projection");
    assert_eq!(restored.0, "link_event_identity_gamma");
    assert_eq!(restored.1, "keyverse_issuer_gamma");
    assert_eq!(restored.2, "keyverse_subject_gamma");
}

#[test]
fn exact_replay_clears_stale_projection_after_unlink() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut unlinked = linked_participant();
    unlinked
        .record_link_end(
            "link_end_event_identity_alpha",
            "unlink_evidence_identity_alpha",
            10_200,
        )
        .unwrap();
    persist_ok(&mut client, &unlinked);
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.current_participant_identity_link (\
                 participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &"participant_identity_alpha",
                &"link_event_identity_alpha",
                &"tenant_identity_alpha",
                &"keyverse_issuer_alpha",
                &"keyverse_subject_alpha",
            ],
        )
        .unwrap();

    assert_eq!(
        persist_ok(&mut client, &unlinked),
        IdentityLinkPersistenceDisposition::Duplicate
    );
    assert!(
        current_projection(&mut client, "participant_identity_alpha").is_none(),
        "unlink replay must drop a stale current projection"
    );
}

#[test]
fn two_unterminated_links_for_one_subject_fail_closed_on_lookup() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.assessment_participant \
             (participant_ref, tenant_ref, created_at_unix_ms) \
             VALUES ($1, $2, $3)",
            &[
                &"participant_identity_beta",
                &"tenant_identity_alpha",
                &10_000_i64,
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.participant_identity_link (\
                 identity_link_ref, participant_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref, anonymous_proof_ref, authenticated_proof_ref, \
                 linked_at_unix_ms\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &"link_event_identity_corrupt",
                &"participant_identity_beta",
                &"tenant_identity_alpha",
                &"keyverse_issuer_alpha",
                &"keyverse_subject_alpha",
                &"anonymous_proof_identity_corrupt",
                &"authenticated_proof_identity_corrupt",
                &10_150_i64,
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = load_participant_by_current_identity_subject(
        &mut transaction,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        IdentityLinkPersistenceError::CorruptHistory
    ));
}

#[test]
fn link_end_cannot_attach_to_another_participants_link() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    persist_ok(&mut client, &anonymous_participant_beta());

    let error = client
        .execute(
            "INSERT INTO identity_link_persistence_test.participant_identity_link_end (\
                 link_end_event_ref, participant_ref, linked_event_ref, evidence_ref, \
                 ended_at_unix_ms\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &"link_end_event_identity_cross",
                &"participant_identity_beta",
                &"link_event_identity_alpha",
                &"unlink_evidence_identity_cross",
                &10_200_i64,
            ],
        )
        .expect_err("a link-end must belong to the same participant as the ended link");
    assert!(error.as_db_error().is_some());
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
fn migration_indexes_history_subject_lookup() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let indexed: bool = client
        .query_one(
            "SELECT EXISTS (\
                 SELECT 1 FROM pg_indexes \
                 WHERE schemaname = 'identity_link_persistence_test' \
                   AND indexname = 'participant_identity_link_current_subject_lookup'\
             )",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        indexed,
        "unterminated-subject lookup must use an indexed history path"
    );
}

#[test]
fn restore_reconcile_rebuilds_missing_and_clears_stale_projections() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &relinked_participant());
    persist_ok(&mut client, &linked_participant_beta());
    drop_current_projection(&mut client);
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.current_participant_identity_link (\
                 participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &"participant_identity_alpha",
                &"link_event_identity_alpha",
                &"tenant_identity_alpha",
                &"keyverse_issuer_alpha",
                &"keyverse_subject_alpha",
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let restored = reconcile_identity_link_current_projections(&mut transaction)
        .expect("restore reconcile must rebuild current projections from unterminated history");
    transaction.commit().unwrap();
    assert_eq!(restored, 2);

    let alpha = current_projection(&mut client, "participant_identity_alpha")
        .expect("relinked participant must keep only the current account after restore");
    assert_eq!(alpha.0, "link_event_identity_gamma");
    assert_eq!(alpha.1, "keyverse_issuer_gamma");
    assert_eq!(alpha.2, "keyverse_subject_gamma");
    let beta = current_projection(&mut client, "participant_identity_beta")
        .expect("linked participant must regain the unique enforcer after restore");
    assert_eq!(beta.0, "link_event_identity_beta");
    assert_eq!(beta.2, "keyverse_subject_beta");

    let mut other = ParticipantRecord::new_anonymous(
        "participant_identity_delta",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap();
    other
        .link_account(
            "link_event_identity_delta",
            "keyverse_issuer_gamma",
            "keyverse_subject_gamma",
            "anonymous_proof_identity_delta",
            "authenticated_proof_identity_delta",
            10_400,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &other),
        IdentityLinkPersistenceError::SubjectAlreadyBound
    ));
}

#[test]
fn restore_reconcile_frees_ended_subject_for_a_new_participant() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &relinked_participant());
    drop_current_projection(&mut client);
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.current_participant_identity_link (\
                 participant_ref, identity_link_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref\
             ) VALUES ($1, $2, $3, $4, $5)",
            &[
                &"participant_identity_alpha",
                &"link_event_identity_alpha",
                &"tenant_identity_alpha",
                &"keyverse_issuer_alpha",
                &"keyverse_subject_alpha",
            ],
        )
        .unwrap();

    let mut rebound = ParticipantRecord::new_anonymous(
        "participant_identity_epsilon",
        "tenant_identity_alpha",
        10_000,
    )
    .unwrap();
    rebound
        .link_account(
            "link_event_identity_epsilon",
            "keyverse_issuer_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_identity_epsilon",
            "authenticated_proof_identity_epsilon",
            10_500,
        )
        .unwrap();
    assert!(
        matches!(
            persist_err(&mut client, &rebound),
            IdentityLinkPersistenceError::SubjectAlreadyBound
        ),
        "a stale current row for an ended subject must block a new account link until restore reconcile runs"
    );

    let mut transaction = client.transaction().unwrap();
    reconcile_identity_link_current_projections(&mut transaction)
        .expect("restore reconcile must clear the stale ended-subject unique enforcer");
    transaction.commit().unwrap();

    assert_eq!(
        persist_ok(&mut client, &rebound),
        IdentityLinkPersistenceDisposition::Inserted
    );
    let recovered = load_by_subject_ok(
        &mut client,
        "tenant_identity_alpha",
        "keyverse_issuer_alpha",
        "keyverse_subject_alpha",
    )
    .expect("ended subject must resolve to the new participant after restore reconcile");
    assert_eq!(recovered.participant_ref(), "participant_identity_epsilon");
    assert_eq!(
        recovered.linked_subject_ref(),
        Some("keyverse_subject_alpha")
    );
}

#[test]
fn restore_reconcile_fails_closed_on_two_unterminated_subjects() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    persist_ok(&mut client, &linked_participant());
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.assessment_participant \
             (participant_ref, tenant_ref, created_at_unix_ms) \
             VALUES ($1, $2, $3)",
            &[
                &"participant_identity_beta",
                &"tenant_identity_alpha",
                &10_000_i64,
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO identity_link_persistence_test.participant_identity_link (\
                 identity_link_ref, participant_ref, tenant_ref, identity_issuer, \
                 identity_subject_ref, anonymous_proof_ref, authenticated_proof_ref, \
                 linked_at_unix_ms\
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &"link_event_identity_corrupt",
                &"participant_identity_beta",
                &"tenant_identity_alpha",
                &"keyverse_issuer_alpha",
                &"keyverse_subject_alpha",
                &"anonymous_proof_identity_corrupt",
                &"authenticated_proof_identity_corrupt",
                &10_150_i64,
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let error = reconcile_identity_link_current_projections(&mut transaction).unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        IdentityLinkPersistenceError::CorruptHistory
    ));
}

#[test]
fn restore_reconcile_rejects_serializable_isolation() {
    let _guard = identity_link_test_guard();
    let mut client = test_client();
    reset_identity_link_tables(&mut client);
    apply_participant_identity_link_migration(&mut client).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    let error = reconcile_identity_link_current_projections(&mut transaction).unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        IdentityLinkPersistenceError::UnsupportedIsolationLevel
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
