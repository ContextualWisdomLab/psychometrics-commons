//! Real `PostgreSQL` contract for consent persistence with transactional outbox evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, ConsentPersistenceDisposition, ConsentPersistenceError,
};
use psychometrics_commons_runtime::postgres_consent_propagation::{
    persist_consent_ledger_with_outbox, ConsentOutboxPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, PersistenceDisposition, PersistenceError,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TENANT_REF: &str = "tenant_consent_outbox_alpha";

static CONSENT_OUTBOX_TEST_LOCK: Mutex<()> = Mutex::new(());

fn consent_outbox_guard() -> MutexGuard<'static, ()> {
    CONSENT_OUTBOX_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_outbox_transaction_test;\
             SET search_path TO consent_outbox_transaction_test;",
        )
        .unwrap();
    client
}

fn reset_and_migrate(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS consent_event;\
             DROP TABLE IF EXISTS consent_ledger;",
        )
        .unwrap();
    apply_integration_migration(client).unwrap();
    apply_consent_migration(client).unwrap();
}

fn research_ledger(event_ref: &str, decision: ConsentDecision) -> ConsentLedger {
    let mut ledger = ConsentLedger::new("participant_consent_outbox_alpha").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref,
            purpose: ConsentPurpose::ResearchContribution,
            decision,
            consent_form_version_ref: "consent_form_research_v1",
            research_scope_ref: Some("research_scope_publication_v1"),
            occurred_at_unix_ms: 10_000,
        })
        .unwrap();
    ledger
}

fn propagation_event(
    event_ref: &str,
    consent_event_ref: &str,
    tenant_ref: &str,
    subject_ref: &str,
    digest: &str,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "consent.research.changed",
        "v1",
        "psychometrics_commons",
        tenant_ref,
        subject_ref,
        10_000,
        "correlation_consent_outbox_alpha",
        Some(consent_event_ref),
        digest,
    )
    .unwrap()
}

#[test]
fn consent_and_outbox_commit_and_replay_together() {
    let _guard = consent_outbox_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let ledger = research_ledger("consent_event_research_grant", ConsentDecision::Granted);
    let event = propagation_event(
        "event_consent_research_grant",
        "consent_event_research_grant",
        TENANT_REF,
        ledger.participant_ref(),
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    let inserted =
        persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &event, 3)
            .unwrap();
    assert_eq!(inserted.consent(), ConsentPersistenceDisposition::Inserted);
    assert_eq!(inserted.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate =
        persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &event, 3)
            .unwrap();
    assert_eq!(
        duplicate.consent(),
        ConsentPersistenceDisposition::Duplicate
    );
    assert_eq!(duplicate.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    let consent_count: i64 = client
        .query_one("SELECT count(*) FROM consent_event", &[])
        .unwrap()
        .get(0);
    let outbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(consent_count, 1);
    assert_eq!(outbox_count, 1);
}

#[test]
fn propagation_envelope_must_bind_tenant_participant_and_exact_consent_event() {
    let _guard = consent_outbox_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let ledger = research_ledger("consent_event_envelope_alpha", ConsentDecision::Revoked);
    let wrong_tenant = propagation_event(
        "event_consent_wrong_tenant",
        "consent_event_envelope_alpha",
        "tenant_consent_outbox_other",
        ledger.participant_ref(),
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &wrong_tenant, 3,),
        Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
    ));
    transaction.rollback().unwrap();

    let wrong_subject = propagation_event(
        "event_consent_wrong_subject",
        "consent_event_envelope_alpha",
        TENANT_REF,
        "participant_other",
        DIGEST_A,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(
            &mut transaction,
            TENANT_REF,
            &ledger,
            &wrong_subject,
            3,
        ),
        Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
    ));
    transaction.rollback().unwrap();

    let wrong_causation = propagation_event(
        "event_consent_wrong_causation",
        "consent_event_other",
        TENANT_REF,
        ledger.participant_ref(),
        DIGEST_A,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(
            &mut transaction,
            TENANT_REF,
            &ledger,
            &wrong_causation,
            3,
        ),
        Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
    ));
    transaction.rollback().unwrap();

    let consent_count: i64 = client
        .query_one("SELECT count(*) FROM consent_event", &[])
        .unwrap()
        .get(0);
    assert_eq!(consent_count, 0);
}

#[test]
fn late_outbox_conflict_rolls_back_new_consent_evidence() {
    let _guard = consent_outbox_guard();
    let mut client = test_client();
    reset_and_migrate(&mut client);
    let ledger = research_ledger("consent_event_conflict_alpha", ConsentDecision::Granted);
    let existing_event = propagation_event(
        "event_consent_conflict_alpha",
        "consent_event_conflict_alpha",
        TENANT_REF,
        ledger.participant_ref(),
        DIGEST_A,
    );
    assert_eq!(
        enqueue_outbox_event(&mut client, &existing_event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let conflicting_event = propagation_event(
        "event_consent_conflict_alpha",
        "consent_event_conflict_alpha",
        TENANT_REF,
        ledger.participant_ref(),
        DIGEST_B,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(
            &mut transaction,
            TENANT_REF,
            &ledger,
            &conflicting_event,
            3,
        ),
        Err(ConsentOutboxPersistenceError::Outbox(
            PersistenceError::ConflictingReplay
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
    assert_eq!(outbox_count, 1);
}

#[test]
fn consent_outbox_errors_retain_typed_sources() {
    let errors = [
        ConsentOutboxPersistenceError::Consent(ConsentPersistenceError::InvalidReference),
        ConsentOutboxPersistenceError::Outbox(PersistenceError::InvalidReference),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
        assert!(error.source().is_some());
    }
    let envelope = ConsentOutboxPersistenceError::InvalidPropagationEnvelope;
    assert!(!envelope.to_string().is_empty());
    assert!(envelope.source().is_none());
}
