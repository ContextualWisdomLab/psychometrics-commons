//! Failure-boundary coverage for consent/outbox transactional composition.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, ConsentPersistenceError,
};
use psychometrics_commons_runtime::postgres_consent_propagation::{
    persist_consent_ledger_with_outbox, ConsentOutboxPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const TENANT_REF: &str = "tenant_consent_boundary_alpha";
static CONSENT_BOUNDARY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn boundary_guard() -> MutexGuard<'static, ()> {
    CONSENT_BOUNDARY_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_outbox_boundary_test;\
             SET search_path TO consent_outbox_boundary_test;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS consent_event;\
             DROP TABLE IF EXISTS consent_ledger;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_consent_migration(&mut client).unwrap();
    client
}

fn ledger_and_event() -> (ConsentLedger, IntegrationEvent) {
    let mut ledger = ConsentLedger::new("participant_consent_boundary_alpha").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_boundary_alpha",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_boundary_v1",
            research_scope_ref: Some("research_scope_boundary_v1"),
            occurred_at_unix_ms: 30_000,
        })
        .unwrap();
    let event = IntegrationEvent::new(
        "event_consent_boundary_alpha",
        "consent.research.changed",
        "v1",
        "psychometrics_commons",
        TENANT_REF,
        ledger.participant_ref(),
        30_000,
        "correlation_consent_boundary_alpha",
        Some("consent_event_boundary_alpha"),
        DIGEST,
    )
    .unwrap();
    (ledger, event)
}

#[test]
fn unsupported_consent_isolation_is_typed_and_enqueues_nothing() {
    let _guard = boundary_guard();
    let mut client = test_client();
    let (ledger, event) = ledger_and_event();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &event, 3),
        Err(ConsentOutboxPersistenceError::Consent(
            ConsentPersistenceError::UnsupportedIsolationLevel
        ))
    ));
    transaction.rollback().unwrap();

    let consent_count: i64 = client
        .query_one("SELECT count(*) FROM consent_event", &[])
        .unwrap()
        .get(0);
    let outbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(consent_count, 0);
    assert_eq!(outbox_count, 0);
}
