//! Real PostgreSQL regression contract for restart-safe consent reconstruction.
//!
//! Purpose-specific consent is append-only evidence. Reload must preserve physical
//! insertion order for same-millisecond decisions and fail closed when stored
//! history cannot be reconstructed without guessing an order.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, load_consent_ledger, persist_consent_ledger,
    ConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static CONSENT_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    CONSENT_RELOAD_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_reload_regression_test;\
             SET search_path TO consent_reload_regression_test;",
        )
        .unwrap();
    client
}

fn reset(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_reload_regression_test.consent_event;\
             DROP TABLE IF EXISTS consent_reload_regression_test.consent_ledger;",
        )
        .unwrap();
    apply_consent_migration(client).unwrap();
}

fn persist(client: &mut Client, ledger: &ConsentLedger) {
    let mut transaction = client.transaction().unwrap();
    persist_consent_ledger(&mut transaction, ledger).unwrap();
    transaction.commit().unwrap();
}

fn load(client: &mut Client, participant_ref: &str) -> Option<ConsentLedger> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_consent_ledger(&mut transaction, participant_ref).unwrap();
    transaction.commit().unwrap();
    loaded
}

fn research_event<'a>(
    event_ref: &'a str,
    decision: ConsentDecision,
    occurred_at_unix_ms: u64,
) -> ConsentEventInput<'a> {
    ConsentEventInput {
        event_ref,
        purpose: ConsentPurpose::ResearchContribution,
        decision,
        consent_form_version_ref: "consent_form_research_v1",
        research_scope_ref: Some("research_scope_research_v1"),
        occurred_at_unix_ms,
    }
}

#[test]
fn same_millisecond_revoke_remains_latest_after_restart() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut grant_only = ConsentLedger::new("participant_consent_reload_alpha").unwrap();
    grant_only
        .record(research_event(
            "consent_event_zzz_grant",
            ConsentDecision::Granted,
            32_000,
        ))
        .unwrap();
    persist(&mut client, &grant_only);

    let mut revoked = grant_only.clone();
    revoked
        .record(research_event(
            "consent_event_aaa_revoke",
            ConsentDecision::Revoked,
            32_000,
        ))
        .unwrap();
    persist(&mut client, &revoked);

    let loaded = load(&mut client, "participant_consent_reload_alpha")
        .expect("persisted consent ledger must survive restart");
    let snapshot = loaded.snapshot_as("consent_snapshot_reload_alpha").unwrap();

    assert_eq!(loaded, revoked);
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert_eq!(snapshot.active_research_scope(), None);
}

#[test]
fn non_monotonic_stored_history_fails_closed_instead_of_reordering() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    persist(
        &mut client,
        &ConsentLedger::new("participant_consent_reload_corrupt").unwrap(),
    );
    client
        .execute(
            "INSERT INTO consent_event (\
                 participant_ref, event_ref, consent_purpose, consent_decision, \
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
             ) VALUES ($1, $2, 'service_operation', 'granted', $3, NULL, 20000)",
            &[
                &"participant_consent_reload_corrupt",
                &"consent_event_later",
                &"consent_form_service_v1",
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO consent_event (\
                 participant_ref, event_ref, consent_purpose, consent_decision, \
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
             ) VALUES ($1, $2, 'service_operation', 'revoked', $3, NULL, 19000)",
            &[
                &"participant_consent_reload_corrupt",
                &"consent_event_earlier",
                &"consent_form_service_v1",
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut transaction, "participant_consent_reload_corrupt"),
        Err(ConsentPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn ambiguous_physical_order_and_noncanonical_participant_alias_fail_closed() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut grant_only = ConsentLedger::new("participant_consent_reload_tie").unwrap();
    grant_only
        .record(research_event(
            "consent_event_zzz_grant_tie",
            ConsentDecision::Granted,
            41_000,
        ))
        .unwrap();
    persist(&mut client, &grant_only);

    let mut revoked = grant_only.clone();
    revoked
        .record(research_event(
            "consent_event_aaa_revoke_tie",
            ConsentDecision::Revoked,
            41_000,
        ))
        .unwrap();
    persist(&mut client, &revoked);

    client
        .execute(
            "UPDATE consent_event \
             SET created_at = TIMESTAMPTZ '2026-08-21 00:00:00+00' \
             WHERE participant_ref = $1",
            &[&"participant_consent_reload_tie"],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut transaction, "participant_consent_reload_tie"),
        Err(ConsentPersistenceError::CorruptHistory)
    ));
    transaction.rollback().unwrap();

    let mut alias_transaction = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(
            &mut alias_transaction,
            " participant_consent_reload_tie"
        ),
        Err(ConsentPersistenceError::InvalidReference)
    ));
    alias_transaction.rollback().unwrap();
}
