//! Authorized consent persist must refuse a foreign actor before any durable row exists.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, ConsentPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_consent_authorization::{
    persist_authorized_consent_ledger, AuthorizedConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const TENANT_REF: &str = "tenant_consent_authorized_persist";
const PARTICIPANT_REF: &str = "participant_consent_authorized_persist";

static AUTHORIZED_PERSIST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    AUTHORIZED_PERSIST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_authorized_persist_test;\
             SET search_path TO consent_authorized_persist_test;",
        )
        .unwrap();
    client
}

fn reset_consent_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_authorized_persist_test.consent_event;\
             DROP TABLE IF EXISTS consent_authorized_persist_test.consent_ledger;",
        )
        .unwrap();
}

fn owner_context() -> AuthorizationContext {
    AuthorizationContext::new(
        TENANT_REF,
        "subject_consent_authorized_persist",
        Some(PARTICIPANT_REF),
        &[ProductRole::Participant],
    )
    .unwrap()
}

fn research_grant_ledger() -> ConsentLedger {
    let mut ledger = ConsentLedger::new(PARTICIPANT_REF).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_authorized_persist_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_authorized_persist_v1",
            research_scope_ref: Some("research_scope_authorized_persist"),
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
fn foreign_participant_cannot_persist_another_consent_ledger() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_consent_authorized_intruder",
        Some("participant_consent_authorized_other"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let ledger = research_grant_ledger();
    let mut transaction = client.transaction().unwrap();
    let error = persist_authorized_consent_ledger(&actor, &ledger, TENANT_REF, &mut transaction)
        .expect_err("foreign participant must not persist another ledger");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::Authorization(AuthorizationError::OwnerMismatch)
    ));
    transaction.commit().unwrap();

    assert_eq!(ledger_count(&mut client), 0);
    assert_eq!(event_count(&mut client), 0);
}

#[test]
fn owner_persist_inserts_the_authorized_research_grant() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = research_grant_ledger();
    let mut transaction = client.transaction().unwrap();
    let disposition =
        persist_authorized_consent_ledger(&owner_context(), &ledger, TENANT_REF, &mut transaction)
            .unwrap();
    transaction.commit().unwrap();

    assert_eq!(disposition, ConsentPersistenceDisposition::Inserted);
    assert_eq!(ledger_count(&mut client), 1);
    assert_eq!(event_count(&mut client), 1);
}
