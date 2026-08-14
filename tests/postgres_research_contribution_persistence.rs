//! Real `PostgreSQL` contract for consent-bound research-contribution evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentSnapshot,
    ResearchContribution,
};
use psychometrics_commons_runtime::postgres_research_contribution::{
    apply_research_contribution_migration, persist_research_contribution,
    ResearchContributionPersistenceDisposition, ResearchContributionPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static RESEARCH_CONTRIBUTION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    RESEARCH_CONTRIBUTION_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS research_contribution_test;\
             SET search_path TO research_contribution_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS research_contribution_test.research_withdrawal_event;\
             DROP TABLE IF EXISTS research_contribution_test.research_contribution;",
        )
        .unwrap();
}

fn research_snapshot(
    participant_ref: &str,
    snapshot_ref: &str,
    research_scope_ref: &str,
    occurred_at_unix_ms: u64,
) -> ConsentSnapshot {
    let mut ledger = ConsentLedger::new(participant_ref).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_consent_form_v1",
            research_scope_ref: Some(research_scope_ref),
            occurred_at_unix_ms,
        })
        .unwrap();
    ledger.snapshot_as(snapshot_ref).unwrap()
}

fn contribution<'a>(
    contribution_ref: &str,
    research_participant_ref: &str,
    snapshot: &'a ConsentSnapshot,
    started_at_unix_ms: u64,
) -> ResearchContribution {
    ResearchContribution::from_snapshot(
        contribution_ref,
        research_participant_ref,
        snapshot,
        started_at_unix_ms,
    )
    .unwrap()
}

fn persist_ok(
    client: &mut Client,
    snapshot: &ConsentSnapshot,
    contribution: &ResearchContribution,
) -> ResearchContributionPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_research_contribution(&mut transaction, snapshot, contribution).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    snapshot: &ConsentSnapshot,
    contribution: &ResearchContribution,
) -> ResearchContributionPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_research_contribution(&mut transaction, snapshot, contribution).unwrap_err();
    transaction.rollback().unwrap();
    error
}

#[test]
fn active_contribution_and_withdrawal_are_append_only_and_idempotent() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_alpha",
        "consent_snapshot_research_alpha",
        "research_scope_alpha",
        1_000,
    );
    let active = contribution(
        "research_contribution_alpha",
        "research_participant_alpha",
        &snapshot,
        1_100,
    );

    assert_eq!(
        persist_ok(&mut client, &snapshot, &active),
        ResearchContributionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot, &active),
        ResearchContributionPersistenceDisposition::Duplicate
    );

    let withdrawn = active.withdraw("research_withdrawal_alpha", 2_000).unwrap();
    assert_eq!(
        persist_ok(&mut client, &snapshot, &withdrawn),
        ResearchContributionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &snapshot, &withdrawn),
        ResearchContributionPersistenceDisposition::Duplicate
    );

    assert_eq!(
        persist_ok(&mut client, &snapshot, &active),
        ResearchContributionPersistenceDisposition::Duplicate,
        "replaying original opt-in evidence must never erase a durable withdrawal"
    );
    let row = client
        .query_one(
            "SELECT withdrawal_event_ref, withdrawn_at_unix_ms \
             FROM research_withdrawal_event WHERE contribution_ref = $1",
            &[&active.contribution_ref()],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "research_withdrawal_alpha");
    assert_eq!(row.get::<_, i64>(1), 2_000);
}

#[test]
fn immutable_contribution_identity_rebinding_fails_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let first_snapshot = research_snapshot(
        "participant_research_beta",
        "consent_snapshot_research_beta",
        "research_scope_beta",
        3_000,
    );
    let first = contribution(
        "research_contribution_beta",
        "research_participant_beta",
        &first_snapshot,
        3_100,
    );
    assert_eq!(
        persist_ok(&mut client, &first_snapshot, &first),
        ResearchContributionPersistenceDisposition::Inserted
    );

    let rebound_snapshot = research_snapshot(
        "participant_research_gamma",
        "consent_snapshot_research_gamma",
        "research_scope_gamma",
        3_000,
    );
    let rebound = contribution(
        "research_contribution_beta",
        "research_participant_gamma",
        &rebound_snapshot,
        3_200,
    );
    assert!(matches!(
        persist_err(&mut client, &rebound_snapshot, &rebound),
        ResearchContributionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn supplied_consent_snapshot_must_match_exact_authorizing_snapshot_and_scope() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let authorizing_snapshot = research_snapshot(
        "participant_research_delta",
        "consent_snapshot_research_delta",
        "research_scope_delta",
        4_000,
    );
    let contribution = contribution(
        "research_contribution_delta",
        "research_participant_delta",
        &authorizing_snapshot,
        4_100,
    );
    let wrong_snapshot = research_snapshot(
        "participant_research_delta",
        "consent_snapshot_research_wrong",
        "research_scope_wrong",
        4_000,
    );

    assert!(matches!(
        persist_err(&mut client, &wrong_snapshot, &contribution),
        ResearchContributionPersistenceError::ConsentSnapshotMismatch
    ));
    assert_eq!(
        client
            .query_one("SELECT count(*)::bigint FROM research_contribution", &[])
            .unwrap()
            .get::<_, i64>(0),
        0
    );
}

#[test]
fn operational_and_research_identity_namespace_reuse_fails_closed_at_adapter_and_database() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let original_snapshot = research_snapshot(
        "participant_research_epsilon",
        "consent_snapshot_shared_epsilon",
        "research_scope_epsilon",
        5_000,
    );
    let contribution = contribution(
        "research_contribution_epsilon",
        "research_participant_epsilon",
        &original_snapshot,
        5_100,
    );
    let substituted_snapshot = research_snapshot(
        "research_participant_epsilon",
        "consent_snapshot_shared_epsilon",
        "research_scope_epsilon",
        5_000,
    );
    assert!(matches!(
        persist_err(&mut client, &substituted_snapshot, &contribution),
        ResearchContributionPersistenceError::OperationalIdentityReuse
    ));

    let direct_insert = client.execute(
        "INSERT INTO research_contribution (\
             contribution_ref, participant_ref, research_participant_ref, \
             consent_snapshot_ref, research_scope_ref, started_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6)",
        &[
            &"research_contribution_db_guard",
            &"same_identity_ref",
            &"same_identity_ref",
            &"consent_snapshot_db_guard",
            &"research_scope_db_guard",
            &5_200_i64,
        ],
    );
    assert!(direct_insert.is_err());
}

#[test]
fn withdrawal_identity_cannot_be_reused_or_rebound() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let first_snapshot = research_snapshot(
        "participant_research_zeta",
        "consent_snapshot_research_zeta",
        "research_scope_zeta",
        6_000,
    );
    let first = contribution(
        "research_contribution_zeta",
        "research_participant_zeta",
        &first_snapshot,
        6_100,
    );
    let first_withdrawal = first.withdraw("research_withdrawal_shared", 6_500).unwrap();
    persist_ok(&mut client, &first_snapshot, &first_withdrawal);

    let conflicting_withdrawal = first.withdraw("research_withdrawal_other", 6_600).unwrap();
    assert!(matches!(
        persist_err(&mut client, &first_snapshot, &conflicting_withdrawal),
        ResearchContributionPersistenceError::ConflictingReplay
    ));

    let second_snapshot = research_snapshot(
        "participant_research_eta",
        "consent_snapshot_research_eta",
        "research_scope_eta",
        7_000,
    );
    let second = contribution(
        "research_contribution_eta",
        "research_participant_eta",
        &second_snapshot,
        7_100,
    );
    let reused_event = second.withdraw("research_withdrawal_shared", 7_500).unwrap();
    assert!(matches!(
        persist_err(&mut client, &second_snapshot, &reused_event),
        ResearchContributionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn tampered_stored_contribution_is_detected_on_replay() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_theta",
        "consent_snapshot_research_theta",
        "research_scope_theta",
        8_000,
    );
    let contribution = contribution(
        "research_contribution_theta",
        "research_participant_theta",
        &snapshot,
        8_100,
    );
    persist_ok(&mut client, &snapshot, &contribution);
    client
        .execute(
            "UPDATE research_contribution SET research_scope_ref = $1 \
             WHERE contribution_ref = $2",
            &[&"research_scope_tampered", &contribution.contribution_ref()],
        )
        .unwrap();

    assert!(matches!(
        persist_err(&mut client, &snapshot, &contribution),
        ResearchContributionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn persistence_requires_read_committed_isolation() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_iota",
        "consent_snapshot_research_iota",
        "research_scope_iota",
        9_000,
    );
    let contribution = contribution(
        "research_contribution_iota",
        "research_participant_iota",
        &snapshot,
        9_100,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_research_contribution(&mut transaction, &snapshot, &contribution),
        Err(ResearchContributionPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_relation_surfaces_typed_database_failure() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);

    let snapshot = research_snapshot(
        "participant_research_kappa",
        "consent_snapshot_research_kappa",
        "research_scope_kappa",
        10_000,
    );
    let contribution = contribution(
        "research_contribution_kappa",
        "research_participant_kappa",
        &snapshot,
        10_100,
    );
    assert!(matches!(
        persist_err(&mut client, &snapshot, &contribution),
        ResearchContributionPersistenceError::Database(_)
    ));
}

#[test]
fn persistence_error_messages_are_stable_and_non_sensitive() {
    let variants = [
        ResearchContributionPersistenceError::InvalidReference,
        ResearchContributionPersistenceError::ConsentSnapshotMismatch,
        ResearchContributionPersistenceError::OperationalIdentityReuse,
        ResearchContributionPersistenceError::InvalidTimestamp,
        ResearchContributionPersistenceError::InvalidLifecycleEvidence,
        ResearchContributionPersistenceError::ConflictingReplay,
        ResearchContributionPersistenceError::UnsupportedIsolationLevel,
    ];
    for error in variants {
        let message = error.to_string();
        assert!(!message.is_empty());
        assert!(!message.contains("participant_research"));
    }
}
