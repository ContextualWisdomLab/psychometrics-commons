//! Consent propagation must be bound to the latest accepted consent change, not stale history.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, persist_consent_ledger,
};
use psychometrics_commons_runtime::postgres_consent_propagation::{
    persist_consent_ledger_with_outbox, ConsentOutboxPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const SCHEMA: &str = "consent_outbox_latest_event_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x434F_4E53_4C41_5445;
const DIGEST: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const TENANT_REF: &str = "tenant_consent_latest_alpha";

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL consent-latest-event lock should be acquired");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated consent-latest-event schema should be reset");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_consent_migration(&mut client).expect("consent migration should apply");
    client
}

fn grant_event_input() -> ConsentEventInput<'static> {
    ConsentEventInput {
        event_ref: "consent_event_grant_alpha",
        purpose: ConsentPurpose::ResearchContribution,
        decision: ConsentDecision::Granted,
        consent_form_version_ref: "consent_form_latest_v1",
        research_scope_ref: Some("research_scope_latest_alpha"),
        occurred_at_unix_ms: 30_000,
    }
}

fn ledger_with_grant_only() -> ConsentLedger {
    let mut ledger = ConsentLedger::new("participant_consent_latest_alpha").unwrap();
    ledger.record(grant_event_input()).unwrap();
    ledger
}

fn ledger_with_revocation() -> ConsentLedger {
    let mut ledger = ledger_with_grant_only();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_revoke_alpha",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "consent_form_latest_v1",
            research_scope_ref: Some("research_scope_latest_alpha"),
            occurred_at_unix_ms: 31_000,
        })
        .unwrap();
    ledger
}

fn propagation_event(
    event_ref: &str,
    causation_ref: &str,
    occurred_at_unix_ms: u64,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "consent.research.changed",
        "v1",
        "psychometrics_commons",
        TENANT_REF,
        "participant_consent_latest_alpha",
        occurred_at_unix_ms,
        "correlation_consent_latest_alpha",
        Some(causation_ref),
        DIGEST,
    )
    .unwrap()
}

#[test]
fn stale_grant_cannot_be_propagated_after_a_later_revocation() {
    let mut client = ready_client();
    let ledger = ledger_with_revocation();
    let stale_event = propagation_event(
        "event_consent_stale_grant",
        "consent_event_grant_alpha",
        30_000,
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &stale_event, 3,),
        Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
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

    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .unwrap();
}

#[test]
fn durable_revoke_rejects_grant_only_snapshot_propagation() {
    let mut client = ready_client();
    let mut persist_transaction = client.transaction().unwrap();
    persist_consent_ledger(&mut persist_transaction, &ledger_with_revocation())
        .expect("grant then revoke should persist as append-only consent evidence");
    persist_transaction.commit().unwrap();

    let stale_event = propagation_event(
        "event_consent_stale_grant_after_durable_revoke",
        "consent_event_grant_alpha",
        30_000,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger_with_outbox(
            &mut transaction,
            TENANT_REF,
            &ledger_with_grant_only(),
            &stale_event,
            3,
        ),
        Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
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
    assert_eq!(consent_count, 2);
    assert_eq!(outbox_count, 0);

    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .unwrap();
}

#[test]
fn latest_revocation_can_be_persisted_with_its_propagation_event() {
    let mut client = ready_client();
    let ledger = ledger_with_revocation();
    let latest_event = propagation_event(
        "event_consent_latest_revocation",
        "consent_event_revoke_alpha",
        31_000,
    );

    let mut transaction = client.transaction().unwrap();
    persist_consent_ledger_with_outbox(&mut transaction, TENANT_REF, &ledger, &latest_event, 3)
        .expect("latest consent change should persist with its bound outbox event");
    transaction.commit().unwrap();

    let consent_count: i64 = client
        .query_one("SELECT count(*) FROM consent_event", &[])
        .unwrap()
        .get(0);
    let outbox = client
        .query_one(
            "SELECT source_ref, tenant_ref, subject_ref, causation_ref, occurred_at_unix_ms \
             FROM integration_outbox",
            &[],
        )
        .unwrap();
    let source_ref: String = outbox.get(0);
    let tenant_ref: String = outbox.get(1);
    let subject_ref: String = outbox.get(2);
    let causation_ref: Option<String> = outbox.get(3);
    let occurred_at_unix_ms: i64 = outbox.get(4);
    assert_eq!(consent_count, 2);
    assert_eq!(source_ref, "psychometrics_commons");
    assert_eq!(tenant_ref, TENANT_REF);
    assert_eq!(subject_ref, "participant_consent_latest_alpha");
    assert_eq!(causation_ref.as_deref(), Some("consent_event_revoke_alpha"));
    assert_eq!(occurred_at_unix_ms, 31_000);

    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .unwrap();
}
