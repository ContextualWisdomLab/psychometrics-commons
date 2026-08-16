//! Regression contract for durable research-consent snapshot binding.
//!
//! A `ResearchContribution` does not itself carry the operational participant
//! reference. Persistence therefore must resolve that identity only from a
//! previously persisted, immutable research-consent snapshot projection. Pairing
//! a contribution with an arbitrary in-memory snapshot at write time would allow
//! a same-reference/same-scope snapshot collision to rebind operational identity.

use postgres::{Client, NoTls};
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
use std::sync::{Mutex, MutexGuard};

static AUTHORIZATION_BINDING_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    AUTHORIZATION_BINDING_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS research_authorization_binding_test;\
             SET search_path TO research_authorization_binding_test;\
             DROP TABLE IF EXISTS research_withdrawal_event;\
             DROP TABLE IF EXISTS research_contribution;\
             DROP TABLE IF EXISTS research_consent_snapshot;\
             DROP TABLE IF EXISTS consent_event;\
             DROP TABLE IF EXISTS consent_ledger;",
        )
        .unwrap();
    client
}

fn research_grant(
    participant_ref: &str,
    snapshot_ref: &str,
    research_scope_ref: &str,
) -> (ConsentLedger, ConsentSnapshot) {
    let mut ledger = ConsentLedger::new(participant_ref).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_authorization_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_authorization_form_v1",
            research_scope_ref: Some(research_scope_ref),
            occurred_at_unix_ms: 1_000,
        })
        .unwrap();
    let snapshot = ledger.snapshot_as(snapshot_ref).unwrap();
    (ledger, snapshot)
}

#[test]
fn contribution_uses_previously_persisted_snapshot_binding_not_a_write_time_snapshot() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (authoritative_ledger, authoritative) = research_grant(
        "participant_authoritative_alpha",
        "consent_snapshot_collision_alpha",
        "research_scope_collision_alpha",
    );
    let (_colliding_ledger, colliding) = research_grant(
        "participant_wrong_alpha",
        "consent_snapshot_collision_alpha",
        "research_scope_collision_alpha",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_consent_ledger(&mut transaction, &authoritative_ledger).unwrap();
        transaction.commit().unwrap();
    }
    let contribution = ResearchContribution::from_snapshot(
        "research_contribution_binding_alpha",
        "research_participant_binding_alpha",
        &authoritative,
        1_100,
    )
    .unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_research_consent_snapshot(&mut transaction, &authoritative).unwrap(),
            ResearchContributionPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_research_consent_snapshot(&mut transaction, &colliding),
            Err(ResearchContributionPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_research_contribution(&mut transaction, &contribution).unwrap(),
            ResearchContributionPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT participant_ref FROM research_contribution \
             WHERE contribution_ref = $1",
            &[&contribution.contribution_ref()],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "participant_authoritative_alpha");
}

#[test]
fn contribution_fails_closed_until_authorizing_snapshot_projection_is_durable() {
    let _guard = test_guard();
    let mut client = test_client();
    apply_consent_migration(&mut client).unwrap();
    apply_research_contribution_migration(&mut client).unwrap();

    let (ledger, snapshot) = research_grant(
        "participant_authoritative_beta",
        "consent_snapshot_binding_beta",
        "research_scope_binding_beta",
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_consent_ledger(&mut transaction, &ledger).unwrap();
        transaction.commit().unwrap();
    }
    let contribution = ResearchContribution::from_snapshot(
        "research_contribution_binding_beta",
        "research_participant_binding_beta",
        &snapshot,
        1_100,
    )
    .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_research_contribution(&mut transaction, &contribution),
        Err(ResearchContributionPersistenceError::ConsentSnapshotMissing)
    ));
    transaction.rollback().unwrap();
}
