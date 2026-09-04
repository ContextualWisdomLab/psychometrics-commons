//! Anonymous consent persist must refuse an expired or foreign session before any durable row exists.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, ConsentPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_consent_authorization::{
    persist_authorized_anonymous_consent_ledger, AuthorizedConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const TENANT_REF: &str = "tenant_consent_anonymous_persist";
const PARTICIPANT_REF: &str = "participant_consent_anonymous_persist";
const SESSION_REF: &str = "session_consent_anonymous_persist";
const EVIDENCE_REF: &str = "evidence_consent_anonymous_persist";
const VALID_UNTIL_UNIX_MS: u64 = 90_000;

static ANONYMOUS_PERSIST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    ANONYMOUS_PERSIST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_anonymous_persist_test;\
             SET search_path TO consent_anonymous_persist_test;",
        )
        .unwrap();
    client
}

fn reset_consent_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_anonymous_persist_test.consent_event;\
             DROP TABLE IF EXISTS consent_anonymous_persist_test.consent_ledger;",
        )
        .unwrap();
}

fn owner_anonymous_session() -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        TENANT_REF,
        PARTICIPANT_REF,
        SESSION_REF,
        EVIDENCE_REF,
        VALID_UNTIL_UNIX_MS,
    )
    .unwrap()
}

fn research_grant_ledger() -> ConsentLedger {
    let mut ledger = ConsentLedger::new(PARTICIPANT_REF).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_anonymous_persist_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_anonymous_persist_v1",
            research_scope_ref: Some("research_scope_anonymous_persist"),
            occurred_at_unix_ms: 50_000,
        })
        .unwrap();
    ledger
}

fn event_count(client: &mut Client) -> i64 {
    client
        .query_one("SELECT COUNT(*) FROM consent_event", &[])
        .unwrap()
        .get(0)
}

fn ledger_count(client: &mut Client) -> i64 {
    client
        .query_one("SELECT COUNT(*) FROM consent_ledger", &[])
        .unwrap()
        .get(0)
}

#[test]
fn expired_anonymous_session_cannot_persist_a_consent_ledger() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = research_grant_ledger();
    let mut transaction = client.transaction().unwrap();
    let error = persist_authorized_anonymous_consent_ledger(
        &owner_anonymous_session(),
        &ledger,
        VALID_UNTIL_UNIX_MS,
        &mut transaction,
    )
    .expect_err("expired anonymous session must not persist");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::AnonymousSessionExpired
    ));
    transaction.commit().unwrap();

    assert_eq!(ledger_count(&mut client), 0);
    assert_eq!(event_count(&mut client), 0);
}

#[test]
fn anonymous_session_cannot_persist_a_foreign_consent_ledger() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let mut foreign_ledger = ConsentLedger::new("participant_consent_anonymous_other").unwrap();
    foreign_ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_anonymous_persist_foreign",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_anonymous_persist_v1",
            research_scope_ref: Some("research_scope_anonymous_persist"),
            occurred_at_unix_ms: 50_000,
        })
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = persist_authorized_anonymous_consent_ledger(
        &owner_anonymous_session(),
        &foreign_ledger,
        50_000,
        &mut transaction,
    )
    .expect_err("foreign ledger must not persist");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::AnonymousBindingMismatch
    ));
    transaction.commit().unwrap();

    assert_eq!(ledger_count(&mut client), 0);
    assert_eq!(event_count(&mut client), 0);
}

#[test]
fn current_anonymous_session_persists_the_authorized_research_grant() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = research_grant_ledger();
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_authorized_anonymous_consent_ledger(
        &owner_anonymous_session(),
        &ledger,
        50_000,
        &mut transaction,
    )
    .unwrap();
    transaction.commit().unwrap();

    assert_eq!(disposition, ConsentPersistenceDisposition::Inserted);
    assert_eq!(ledger_count(&mut client), 1);
    assert_eq!(event_count(&mut client), 1);
}
