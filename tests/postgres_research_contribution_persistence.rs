//! Real `PostgreSQL` contract for authorization-bound research-contribution evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentSnapshot,
    ResearchContribution,
};
use psychometrics_commons_runtime::postgres_research_contribution::{
    apply_research_contribution_migration, persist_research_consent_snapshot,
    persist_research_contribution, ResearchContributionPersistenceDisposition,
    ResearchContributionPersistenceError,
};
use std::error::Error;
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
             DROP TABLE IF EXISTS research_contribution_test.research_contribution;\
             DROP TABLE IF EXISTS research_contribution_test.research_consent_snapshot;",
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

fn non_research_snapshot(participant_ref: &str, snapshot_ref: &str) -> ConsentSnapshot {
    let mut ledger = ConsentLedger::new(participant_ref).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "service_consent_grant",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "service_consent_form_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 1_000,
        })
        .unwrap();
    ledger.snapshot_as(snapshot_ref).unwrap()
}

fn contribution(
    contribution_ref: &str,
    research_participant_ref: &str,
    snapshot: &ConsentSnapshot,
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

fn persist_snapshot_ok(
    client: &mut Client,
    snapshot: &ConsentSnapshot,
) -> ResearchContributionPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_research_consent_snapshot(&mut transaction, snapshot).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_ok(
    client: &mut Client,
    contribution: &ResearchContribution,
) -> ResearchContributionPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_research_contribution(&mut transaction, contribution).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    contribution: &ResearchContribution,
) -> ResearchContributionPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_research_contribution(&mut transaction, contribution).unwrap_err();
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
    assert_eq!(
        persist_snapshot_ok(&mut client, &snapshot),
        ResearchContributionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_snapshot_ok(&mut client, &snapshot),
        ResearchContributionPersistenceDisposition::Duplicate
    );

    let active = contribution(
        "research_contribution_alpha",
        "research_participant_alpha",
        &snapshot,
        1_100,
    );
    assert_eq!(
        persist_ok(&mut client, &active),
        ResearchContributionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &active),
        ResearchContributionPersistenceDisposition::Duplicate
    );

    let withdrawn = active.withdraw("research_withdrawal_alpha", 2_000).unwrap();
    assert_eq!(
        persist_ok(&mut client, &withdrawn),
        ResearchContributionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &withdrawn),
        ResearchContributionPersistenceDisposition::Duplicate
    );
    assert_eq!(
        persist_ok(&mut client, &active),
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
fn durable_consent_snapshot_reference_cannot_be_rebound() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let authoritative = research_snapshot(
        "participant_research_beta",
        "consent_snapshot_collision_beta",
        "research_scope_beta",
        3_000,
    );
    let rebound_participant = research_snapshot(
        "participant_research_gamma",
        "consent_snapshot_collision_beta",
        "research_scope_beta",
        3_000,
    );
    persist_snapshot_ok(&mut client, &authoritative);

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_research_consent_snapshot(&mut transaction, &rebound_participant),
        Err(ResearchContributionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn non_research_snapshot_is_rejected_before_any_binding_is_written() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = non_research_snapshot(
        "participant_research_no_opt_in",
        "consent_snapshot_no_research_opt_in",
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_research_consent_snapshot(&mut transaction, &snapshot),
        Err(ResearchContributionPersistenceError::ResearchConsentRequired)
    ));
    transaction.rollback().unwrap();
    assert_eq!(
        client
            .query_one(
                "SELECT count(*)::bigint FROM research_consent_snapshot",
                &[]
            )
            .unwrap()
            .get::<_, i64>(0),
        0
    );
}

#[test]
fn contribution_requires_preexisting_snapshot_and_exact_scope_binding() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let missing_snapshot = research_snapshot(
        "participant_research_delta",
        "consent_snapshot_missing_delta",
        "research_scope_delta",
        4_000,
    );
    let missing_contribution = contribution(
        "research_contribution_missing_delta",
        "research_participant_delta",
        &missing_snapshot,
        4_100,
    );
    assert!(matches!(
        persist_err(&mut client, &missing_contribution),
        ResearchContributionPersistenceError::ConsentSnapshotMissing
    ));

    let authoritative = research_snapshot(
        "participant_research_epsilon",
        "consent_snapshot_scope_epsilon",
        "research_scope_epsilon",
        5_000,
    );
    persist_snapshot_ok(&mut client, &authoritative);
    let conflicting_scope_snapshot = research_snapshot(
        "participant_research_epsilon",
        "consent_snapshot_scope_epsilon",
        "research_scope_other",
        5_000,
    );
    let conflicting_scope = contribution(
        "research_contribution_scope_epsilon",
        "research_participant_epsilon",
        &conflicting_scope_snapshot,
        5_100,
    );
    assert!(matches!(
        persist_err(&mut client, &conflicting_scope),
        ResearchContributionPersistenceError::ConsentSnapshotMismatch
    ));
}

#[test]
fn operational_and_research_identity_namespaces_cannot_collapse() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let authoritative = research_snapshot(
        "research_participant_zeta",
        "consent_snapshot_identity_zeta",
        "research_scope_zeta",
        6_000,
    );
    persist_snapshot_ok(&mut client, &authoritative);
    let creation_snapshot = research_snapshot(
        "participant_creation_zeta",
        "consent_snapshot_identity_zeta",
        "research_scope_zeta",
        6_000,
    );
    let contribution = contribution(
        "research_contribution_zeta",
        "research_participant_zeta",
        &creation_snapshot,
        6_100,
    );
    assert!(matches!(
        persist_err(&mut client, &contribution),
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
            &"consent_snapshot_identity_zeta",
            &"research_scope_zeta",
            &6_200_i64,
        ],
    );
    assert!(direct_insert.is_err());
}

#[test]
fn immutable_contribution_and_withdrawal_rebinding_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let first_snapshot = research_snapshot(
        "participant_research_eta",
        "consent_snapshot_research_eta",
        "research_scope_eta",
        7_000,
    );
    persist_snapshot_ok(&mut client, &first_snapshot);
    let first = contribution(
        "research_contribution_shared_eta",
        "research_participant_eta",
        &first_snapshot,
        7_100,
    );
    let first_withdrawal = first
        .withdraw("research_withdrawal_shared_eta", 7_500)
        .unwrap();
    persist_ok(&mut client, &first_withdrawal);

    let conflicting_withdrawal = first
        .withdraw("research_withdrawal_other_eta", 7_600)
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &conflicting_withdrawal),
        ResearchContributionPersistenceError::ConflictingReplay
    ));

    let second_snapshot = research_snapshot(
        "participant_research_theta",
        "consent_snapshot_research_theta",
        "research_scope_theta",
        8_000,
    );
    persist_snapshot_ok(&mut client, &second_snapshot);
    let rebound = contribution(
        "research_contribution_shared_eta",
        "research_participant_theta",
        &second_snapshot,
        8_100,
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        ResearchContributionPersistenceError::ConflictingReplay
    ));

    let second = contribution(
        "research_contribution_theta",
        "research_participant_theta",
        &second_snapshot,
        8_100,
    );
    let reused_withdrawal = second
        .withdraw("research_withdrawal_shared_eta", 8_500)
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &reused_withdrawal),
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
        "participant_research_iota",
        "consent_snapshot_research_iota",
        "research_scope_iota",
        9_000,
    );
    persist_snapshot_ok(&mut client, &snapshot);
    let contribution = contribution(
        "research_contribution_iota",
        "research_participant_iota",
        &snapshot,
        9_100,
    );
    persist_ok(&mut client, &contribution);
    client
        .batch_execute("ALTER TABLE research_contribution DISABLE TRIGGER ALL;")
        .unwrap();
    client
        .execute(
            "UPDATE research_contribution SET started_at_unix_ms = $1 \
             WHERE contribution_ref = $2",
            &[&9_200_i64, &contribution.contribution_ref()],
        )
        .unwrap();
    client
        .batch_execute("ALTER TABLE research_contribution ENABLE TRIGGER ALL;")
        .unwrap();

    assert!(matches!(
        persist_err(&mut client, &contribution),
        ResearchContributionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn oversized_timestamp_and_non_read_committed_isolation_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_kappa",
        "consent_snapshot_research_kappa",
        "research_scope_kappa",
        10_000,
    );
    persist_snapshot_ok(&mut client, &snapshot);
    let oversized = contribution(
        "research_contribution_kappa",
        "research_participant_kappa",
        &snapshot,
        u64::MAX,
    );
    assert!(matches!(
        persist_err(&mut client, &oversized),
        ResearchContributionPersistenceError::InvalidTimestamp
    ));

    let ordinary = contribution(
        "research_contribution_serializable",
        "research_participant_serializable",
        &snapshot,
        10_100,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_research_contribution(&mut transaction, &ordinary),
        Err(ResearchContributionPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_relation_surfaces_typed_database_failure_with_source() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);

    let snapshot = research_snapshot(
        "participant_research_lambda",
        "consent_snapshot_research_lambda",
        "research_scope_lambda",
        11_000,
    );
    let mut transaction = client.transaction().unwrap();
    let error = persist_research_consent_snapshot(&mut transaction, &snapshot).unwrap_err();
    assert!(matches!(
        error,
        ResearchContributionPersistenceError::Database(_)
    ));
    assert!(error.source().is_some());
    transaction.rollback().unwrap();
    assert!(ResearchContributionPersistenceError::InvalidReference
        .source()
        .is_none());
}

#[test]
fn persistence_error_messages_are_stable_and_non_sensitive() {
    let variants = [
        ResearchContributionPersistenceError::InvalidReference,
        ResearchContributionPersistenceError::ResearchConsentRequired,
        ResearchContributionPersistenceError::ConsentSnapshotMissing,
        ResearchContributionPersistenceError::ConsentSnapshotMismatch,
        ResearchContributionPersistenceError::OperationalIdentityReuse,
        ResearchContributionPersistenceError::InvalidTimestamp,
        ResearchContributionPersistenceError::ConflictingReplay,
        ResearchContributionPersistenceError::UnsupportedIsolationLevel,
    ];
    for error in variants {
        let message = error.to_string();
        assert!(!message.is_empty());
        assert!(!message.contains("participant_research"));
    }
}
