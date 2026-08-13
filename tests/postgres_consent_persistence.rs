//! Real `PostgreSQL` contract for durable purpose-specific consent evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, persist_consent_ledger, ConsentPersistenceDisposition,
    ConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static CONSENT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn consent_test_guard() -> MutexGuard<'static, ()> {
    CONSENT_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS consent_persistence_test;\
             SET search_path TO consent_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_consent_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_persistence_test.consent_event;\
             DROP TABLE IF EXISTS consent_persistence_test.consent_ledger;",
        )
        .unwrap();
}

fn grant<'a>(
    event_ref: &'a str,
    purpose: ConsentPurpose,
    form_version_ref: &'a str,
    research_scope_ref: Option<&'a str>,
    occurred_at_unix_ms: u64,
) -> ConsentEventInput<'a> {
    ConsentEventInput {
        event_ref,
        purpose,
        decision: ConsentDecision::Granted,
        consent_form_version_ref: form_version_ref,
        research_scope_ref,
        occurred_at_unix_ms,
    }
}

fn recorded_ledger(participant_ref: &str, inputs: &[ConsentEventInput<'_>]) -> ConsentLedger {
    let mut ledger = ConsentLedger::new(participant_ref).unwrap();
    for input in inputs {
        ledger.record(*input).unwrap();
    }
    ledger
}

#[test]
fn empty_consent_ledger_persist_is_exactly_idempotent() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = ConsentLedger::new("participant_consent_alpha").unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_consent_ledger(&mut transaction, &ledger).unwrap(),
            ConsentPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_consent_ledger(&mut transaction, &ledger).unwrap(),
            ConsentPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }
}

#[test]
fn accepted_consent_events_are_idempotent_and_conflicting_replay_fails_closed() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let first = recorded_ledger(
        "participant_consent_beta",
        &[grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            1_000,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_consent_ledger(&mut transaction, &first).unwrap(),
            ConsentPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_consent_ledger(&mut transaction, &first).unwrap(),
            ConsentPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let conflicting = recorded_ledger(
        "participant_consent_beta",
        &[grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v2",
            None,
            1_000,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &conflicting),
        Err(ConsentPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn later_consent_events_append_and_research_revocation_is_durable() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let granted = recorded_ledger(
        "participant_consent_gamma",
        &[grant(
            "research_grant",
            ConsentPurpose::ResearchContribution,
            "research_form_v1",
            Some("research_scope_v1"),
            2_000,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_consent_ledger(&mut transaction, &granted).unwrap();
        transaction.commit().unwrap();
    }

    let mut revoked = granted.clone();
    revoked
        .record(ConsentEventInput {
            event_ref: "research_revocation",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 2_100,
        })
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_consent_ledger(&mut transaction, &revoked).unwrap(),
            ConsentPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let snapshot = revoked.snapshot_as("consent_snapshot_gamma").unwrap();
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
}

#[test]
fn consent_persistence_requires_read_committed() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = ConsentLedger::new("participant_consent_serializable").unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &ledger),
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn every_consent_purpose_and_decision_persists() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let mut ledger = ConsentLedger::new("participant_consent_all_purposes").unwrap();
    ledger
        .record(grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            3_000,
        ))
        .unwrap();
    ledger
        .record(grant(
            "account_grant",
            ConsentPurpose::AccountPersistence,
            "account_form_v1",
            None,
            3_100,
        ))
        .unwrap();
    ledger
        .record(grant(
            "longitudinal_grant",
            ConsentPurpose::LongitudinalObservation,
            "longitudinal_form_v1",
            None,
            3_200,
        ))
        .unwrap();
    ledger
        .record(grant(
            "communications_grant",
            ConsentPurpose::Communications,
            "communications_form_v1",
            None,
            3_300,
        ))
        .unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "communications_revoke",
            purpose: ConsentPurpose::Communications,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "communications_form_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 3_400,
        })
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_consent_ledger(&mut transaction, &ledger).unwrap(),
        ConsentPersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();
}

#[test]
fn consent_replay_select_failure_is_a_database_failure() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = recorded_ledger(
        "participant_consent_hidden_event",
        &[grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            4_000,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_consent_ledger(&mut transaction, &ledger).unwrap();
        transaction.commit().unwrap();
    }
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS consent_event_failure_sink;\
             CREATE OR REPLACE FUNCTION consent_event_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'consent_event_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER consent_event_redirect_after_insert \
             AFTER INSERT ON consent_event \
             FOR EACH STATEMENT EXECUTE FUNCTION consent_event_redirect_after_insert();",
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &ledger),
        Err(ConsentPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn oversized_event_timestamp_fails_closed_before_insert() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = recorded_ledger(
        "participant_consent_overflow",
        &[grant(
            "service_grant_overflow",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            u64::MAX,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &ledger),
        Err(ConsentPersistenceError::InvalidTimestamp)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_consent_relation_is_a_database_failure() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);

    let ledger = ConsentLedger::new("participant_consent_missing").unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &ledger),
        Err(ConsentPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_consent_event_relation_is_a_database_failure() {
    let _guard = consent_test_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let header = ConsentLedger::new("participant_consent_missing_event").unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_consent_ledger(&mut transaction, &header).unwrap();
        transaction.commit().unwrap();
    }
    client
        .batch_execute("DROP TABLE consent_persistence_test.consent_event;")
        .unwrap();

    let ledger = recorded_ledger(
        "participant_consent_missing_event",
        &[grant(
            "service_grant",
            ConsentPurpose::ServiceOperation,
            "service_form_v1",
            None,
            5_000,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_consent_ledger(&mut transaction, &ledger),
        Err(ConsentPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
