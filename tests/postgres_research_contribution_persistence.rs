//! Real `PostgreSQL` contract for authorization-bound research-contribution evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentSnapshot,
    ResearchContribution,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, persist_consent_ledger,
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
             DROP TABLE IF EXISTS research_contribution_test.research_consent_snapshot;\
             DROP TABLE IF EXISTS research_contribution_test.consent_event;\
             DROP TABLE IF EXISTS research_contribution_test.consent_ledger;",
        )
        .unwrap();
}

fn persist_grant(client: &mut Client, ledger: &ConsentLedger) {
    let mut transaction = client.transaction().unwrap();
    persist_consent_ledger(&mut transaction, ledger).unwrap();
    transaction.commit().unwrap();
}

fn granted_research(
    participant_ref: &str,
    snapshot_ref: &str,
    research_scope_ref: &str,
    occurred_at_unix_ms: u64,
) -> (ConsentLedger, ConsentSnapshot) {
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
    let snapshot = ledger.snapshot_as(snapshot_ref).unwrap();
    (ledger, snapshot)
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

fn granted_snapshot_with_form(
    participant_ref: &str,
    snapshot_ref: &str,
    consent_form_version_ref: &str,
    research_scope_ref: &str,
    occurred_at_unix_ms: u64,
) -> ConsentSnapshot {
    let mut ledger = ConsentLedger::new(participant_ref).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref,
            research_scope_ref: Some(research_scope_ref),
            occurred_at_unix_ms,
        })
        .unwrap();
    ledger.snapshot_as(snapshot_ref).unwrap()
}

fn assert_snapshot_conflicts(client: &mut Client, snapshot: &ConsentSnapshot) {
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_research_consent_snapshot(&mut transaction, snapshot),
        Err(ResearchContributionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

fn tamper_research_contribution(
    client: &mut Client,
    sql: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) {
    client
        .batch_execute("ALTER TABLE research_contribution DISABLE TRIGGER ALL;")
        .unwrap();
    client.execute(sql, params).unwrap();
    client
        .batch_execute("ALTER TABLE research_contribution ENABLE TRIGGER ALL;")
        .unwrap();
}

fn persist_xi_grant(client: &mut Client) -> (ConsentSnapshot, ResearchContribution) {
    let (ledger, snapshot) = granted_research(
        "participant_research_xi",
        "consent_snapshot_research_xi",
        "research_scope_xi",
        14_000,
    );
    persist_grant(client, &ledger);
    persist_snapshot_ok(client, &snapshot);
    let stored = contribution(
        "research_contribution_xi",
        "research_participant_xi",
        &snapshot,
        14_100,
    );
    persist_ok(client, &stored);
    (snapshot, stored)
}

fn assert_conflicting_replay(error: &ResearchContributionPersistenceError) {
    assert!(matches!(
        error,
        ResearchContributionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn active_contribution_and_withdrawal_are_append_only_and_idempotent() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (ledger, snapshot) = granted_research(
        "participant_research_alpha",
        "consent_snapshot_research_alpha",
        "research_scope_alpha",
        1_000,
    );
    persist_grant(&mut client, &ledger);
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (ledger, authoritative) = granted_research(
        "participant_research_beta",
        "consent_snapshot_collision_beta",
        "research_scope_beta",
        3_000,
    );
    persist_grant(&mut client, &ledger);
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
    apply_consent_migration(&mut client).unwrap();
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (missing_ledger, missing_snapshot) = granted_research(
        "participant_research_delta",
        "consent_snapshot_missing_delta",
        "research_scope_delta",
        4_000,
    );
    persist_grant(&mut client, &missing_ledger);
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

    let (authoritative_ledger, authoritative) = granted_research(
        "participant_research_epsilon",
        "consent_snapshot_scope_epsilon",
        "research_scope_epsilon",
        5_000,
    );
    persist_grant(&mut client, &authoritative_ledger);
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (authoritative_ledger, authoritative) = granted_research(
        "research_participant_zeta",
        "consent_snapshot_identity_zeta",
        "research_scope_zeta",
        6_000,
    );
    persist_grant(&mut client, &authoritative_ledger);
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (first_ledger, first_snapshot) = granted_research(
        "participant_research_eta",
        "consent_snapshot_research_eta",
        "research_scope_eta",
        7_000,
    );
    persist_grant(&mut client, &first_ledger);
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

    let (second_ledger, second_snapshot) = granted_research(
        "participant_research_theta",
        "consent_snapshot_research_theta",
        "research_scope_theta",
        8_000,
    );
    persist_grant(&mut client, &second_ledger);
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (ledger, snapshot) = granted_research(
        "participant_research_iota",
        "consent_snapshot_research_iota",
        "research_scope_iota",
        9_000,
    );
    persist_grant(&mut client, &ledger);
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
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (ledger, snapshot) = granted_research(
        "participant_research_kappa",
        "consent_snapshot_research_kappa",
        "research_scope_kappa",
        10_000,
    );
    persist_grant(&mut client, &ledger);
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
    assert_eq!(
        error.to_string(),
        "PostgreSQL research-contribution persistence failed"
    );
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

#[test]
fn revoked_research_grant_blocks_new_contribution_but_preserves_prior_evidence() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (mut ledger, snapshot) = granted_research(
        "participant_research_mu",
        "consent_snapshot_research_mu",
        "research_scope_mu",
        12_000,
    );
    persist_grant(&mut client, &ledger);
    persist_snapshot_ok(&mut client, &snapshot);
    let first = contribution(
        "research_contribution_mu_first",
        "research_participant_mu_first",
        &snapshot,
        12_100,
    );
    assert_eq!(
        persist_ok(&mut client, &first),
        ResearchContributionPersistenceDisposition::Inserted
    );

    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_revoke",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "research_consent_form_v1",
            research_scope_ref: Some("research_scope_mu"),
            occurred_at_unix_ms: 12_200,
        })
        .unwrap();
    persist_grant(&mut client, &ledger);

    let stale = contribution(
        "research_contribution_mu_stale",
        "research_participant_mu_stale",
        &snapshot,
        12_300,
    );
    assert!(
        matches!(
            persist_err(&mut client, &stale),
            ResearchContributionPersistenceError::ResearchConsentRequired
        ),
        "a durable snapshot must not remain an unlimited write capability after revoke"
    );
    assert_eq!(
        persist_ok(&mut client, &first),
        ResearchContributionPersistenceDisposition::Duplicate,
        "revocation must not erase or reject exact replay of already stored evidence"
    );
    let withdrawn = first.withdraw("research_withdrawal_mu", 12_400).unwrap();
    assert_eq!(
        persist_ok(&mut client, &withdrawn),
        ResearchContributionPersistenceDisposition::Inserted,
        "withdrawal after revoke remains allowed because it stops future use"
    );
}

#[test]
fn snapshot_persist_rejects_non_read_committed_isolation() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_nu",
        "consent_snapshot_research_nu",
        "research_scope_nu",
        13_000,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_research_consent_snapshot(&mut transaction, &snapshot),
        Err(ResearchContributionPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn snapshot_and_contribution_replay_rejects_each_field_mismatch() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();
    let (snapshot, stored) = persist_xi_grant(&mut client);

    assert_snapshot_conflicts(
        &mut client,
        &granted_snapshot_with_form(
            "participant_research_xi",
            "consent_snapshot_research_xi",
            "research_consent_form_v1",
            "research_scope_xi_other",
            14_000,
        ),
    );
    assert_snapshot_conflicts(
        &mut client,
        &granted_snapshot_with_form(
            "participant_research_xi",
            "consent_snapshot_research_xi",
            "research_consent_form_v2",
            "research_scope_xi",
            14_000,
        ),
    );
    assert_conflicting_replay(&persist_err(
        &mut client,
        &contribution(
            "research_contribution_xi",
            "research_participant_xi_other",
            &snapshot,
            14_100,
        ),
    ));

    tamper_research_contribution(
        &mut client,
        "UPDATE research_contribution SET consent_snapshot_ref = $1 WHERE contribution_ref = $2",
        &[
            &"consent_snapshot_research_xi_tampered",
            &stored.contribution_ref(),
        ],
    );
    assert_conflicting_replay(&persist_err(&mut client, &stored));

    tamper_research_contribution(
        &mut client,
        "UPDATE research_contribution SET consent_snapshot_ref = $1, \
                research_scope_ref = $2 \
         WHERE contribution_ref = $3",
        &[
            &snapshot.snapshot_ref(),
            &"research_scope_xi_tampered",
            &stored.contribution_ref(),
        ],
    );
    assert_conflicting_replay(&persist_err(&mut client, &stored));

    tamper_research_contribution(
        &mut client,
        "UPDATE research_contribution SET consent_snapshot_ref = $1, \
                research_scope_ref = $2 \
         WHERE contribution_ref = $3",
        &[
            &snapshot.snapshot_ref(),
            &snapshot.active_research_scope().unwrap(),
            &stored.contribution_ref(),
        ],
    );
    persist_ok(
        &mut client,
        &stored.withdraw("research_withdrawal_xi", 14_500).unwrap(),
    );
    assert_conflicting_replay(&persist_err(
        &mut client,
        &stored.withdraw("research_withdrawal_xi", 14_600).unwrap(),
    ));
}

#[test]
fn research_participant_ref_cannot_be_reused_across_operational_identities() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (first_ledger, first_snapshot) = granted_research(
        "participant_research_omicron",
        "consent_snapshot_research_omicron",
        "research_scope_omicron",
        15_000,
    );
    persist_grant(&mut client, &first_ledger);
    persist_snapshot_ok(&mut client, &first_snapshot);
    persist_ok(
        &mut client,
        &contribution(
            "research_contribution_omicron",
            "research_participant_shared",
            &first_snapshot,
            15_100,
        ),
    );

    let (second_ledger, second_snapshot) = granted_research(
        "participant_research_pi",
        "consent_snapshot_research_pi",
        "research_scope_pi",
        15_200,
    );
    persist_grant(&mut client, &second_ledger);
    persist_snapshot_ok(&mut client, &second_snapshot);
    assert!(matches!(
        persist_err(
            &mut client,
            &contribution(
                "research_contribution_pi",
                "research_participant_shared",
                &second_snapshot,
                15_300,
            ),
        ),
        ResearchContributionPersistenceError::OperationalIdentityReuse
    ));
}

#[test]
fn contribution_fails_closed_when_consent_ledger_was_never_persisted() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let snapshot = research_snapshot(
        "participant_research_rho",
        "consent_snapshot_research_rho",
        "research_scope_rho",
        16_000,
    );
    persist_snapshot_ok(&mut client, &snapshot);
    assert!(
        matches!(
            persist_err(
                &mut client,
                &contribution(
                    "research_contribution_rho",
                    "research_participant_rho",
                    &snapshot,
                    16_100,
                ),
            ),
            ResearchContributionPersistenceError::ResearchConsentRequired
        ),
        "a stored snapshot must not authorize a start when no consent_event exists"
    );
}

#[test]
fn later_purpose_level_grant_for_another_scope_blocks_stale_snapshot_start() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (mut ledger, stale_snapshot) = granted_research(
        "participant_research_sigma",
        "consent_snapshot_research_sigma_stale",
        "research_scope_sigma_stale",
        17_000,
    );
    persist_grant(&mut client, &ledger);
    persist_snapshot_ok(&mut client, &stale_snapshot);

    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_grant_replacement",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_consent_form_v1",
            research_scope_ref: Some("research_scope_sigma_current"),
            occurred_at_unix_ms: 17_200,
        })
        .unwrap();
    persist_grant(&mut client, &ledger);
    let current_snapshot = ledger
        .snapshot_as("consent_snapshot_research_sigma_current")
        .unwrap();
    persist_snapshot_ok(&mut client, &current_snapshot);

    assert!(
        matches!(
            persist_err(
                &mut client,
                &contribution(
                    "research_contribution_sigma_stale",
                    "research_participant_sigma_stale",
                    &stale_snapshot,
                    17_300,
                ),
            ),
            ResearchContributionPersistenceError::ResearchConsentRequired
        ),
        "a later purpose-level grant must replace the prior scope as the live write capability"
    );
    assert_eq!(
        persist_ok(
            &mut client,
            &contribution(
                "research_contribution_sigma_current",
                "research_participant_sigma_current",
                &current_snapshot,
                17_400,
            ),
        ),
        ResearchContributionPersistenceDisposition::Inserted
    );
}

#[test]
fn same_millisecond_later_append_replaces_live_scope_when_event_ref_sorts_lower() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let mut ledger = ConsentLedger::new("participant_research_tau").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_grant_z",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_consent_form_v1",
            research_scope_ref: Some("research_scope_tau_stale"),
            occurred_at_unix_ms: 18_000,
        })
        .unwrap();
    persist_grant(&mut client, &ledger);
    let stale_snapshot = ledger
        .snapshot_as("consent_snapshot_research_tau_stale")
        .unwrap();
    persist_snapshot_ok(&mut client, &stale_snapshot);

    ledger
        .record(ConsentEventInput {
            event_ref: "research_consent_grant_a",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_consent_form_v1",
            research_scope_ref: Some("research_scope_tau_current"),
            occurred_at_unix_ms: 18_000,
        })
        .unwrap();
    persist_grant(&mut client, &ledger);
    let current_snapshot = ledger
        .snapshot_as("consent_snapshot_research_tau_current")
        .unwrap();
    persist_snapshot_ok(&mut client, &current_snapshot);

    assert_eq!(
        current_snapshot.active_research_scope(),
        Some("research_scope_tau_current")
    );
    assert!(
        matches!(
            persist_err(
                &mut client,
                &contribution(
                    "research_contribution_tau_stale",
                    "research_participant_tau_stale",
                    &stale_snapshot,
                    18_100,
                ),
            ),
            ResearchContributionPersistenceError::ResearchConsentRequired
        ),
        "append order, not event_ref sort, must choose the live research scope"
    );
    assert_eq!(
        persist_ok(
            &mut client,
            &contribution(
                "research_contribution_tau_current",
                "research_participant_tau_current",
                &current_snapshot,
                18_100,
            ),
        ),
        ResearchContributionPersistenceDisposition::Inserted
    );
}

#[test]
fn operational_participant_cannot_reuse_an_existing_research_participant_ref() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (first_ledger, first_snapshot) = granted_research(
        "participant_research_upsilon",
        "consent_snapshot_research_upsilon",
        "research_scope_upsilon",
        19_000,
    );
    persist_grant(&mut client, &first_ledger);
    persist_snapshot_ok(&mut client, &first_snapshot);
    persist_ok(
        &mut client,
        &contribution(
            "research_contribution_upsilon",
            "research_participant_shared_upsilon",
            &first_snapshot,
            19_100,
        ),
    );

    let (reverse_ledger, reverse_snapshot) = granted_research(
        "research_participant_shared_upsilon",
        "consent_snapshot_research_upsilon_reverse",
        "research_scope_upsilon_reverse",
        19_200,
    );
    persist_grant(&mut client, &reverse_ledger);
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            persist_research_consent_snapshot(&mut transaction, &reverse_snapshot),
            Err(ResearchContributionPersistenceError::OperationalIdentityReuse)
        ),
        "a research participant reference must not later become an operational participant"
    );
    transaction.rollback().unwrap();
}
