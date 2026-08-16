//! Real `PostgreSQL` contract: purpose-specific consent survives process restart.
//!
//! A buyer who grants research contribution and later revokes it in the same
//! millisecond must see the revocation after the runtime reloads the durable
//! ledger. Reload must not invent consent, break insertion-time ties by
//! opaque identity, or accept a stronger isolation level that can hide a
//! concurrent append.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, load_consent_ledger, persist_consent_ledger,
    ConsentPersistenceDisposition, ConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static CONSENT_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn consent_reload_guard() -> MutexGuard<'static, ()> {
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
            "CREATE SCHEMA IF NOT EXISTS consent_reload_test;\
             SET search_path TO consent_reload_test;",
        )
        .unwrap();
    client
}

fn reset_consent_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_reload_test.consent_event;\
             DROP TABLE IF EXISTS consent_reload_test.consent_ledger;",
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

fn persist_ok(client: &mut Client, ledger: &ConsentLedger) -> ConsentPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_consent_ledger(&mut transaction, ledger).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn load_ok(client: &mut Client, participant_ref: &str) -> Option<ConsentLedger> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_consent_ledger(&mut transaction, participant_ref).unwrap();
    transaction.commit().unwrap();
    loaded
}

#[test]
fn unknown_participant_reload_is_absent_not_an_empty_grant() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    assert!(
        load_ok(&mut client, "participant_consent_reload_unknown").is_none(),
        "a participant who never persisted consent must not appear granted after restart"
    );
}

#[test]
fn empty_persisted_ledger_reloads_without_inventing_events() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let empty = ConsentLedger::new("participant_consent_reload_empty").unwrap();
    assert_eq!(
        persist_ok(&mut client, &empty),
        ConsentPersistenceDisposition::Inserted
    );
    let loaded = load_ok(&mut client, "participant_consent_reload_empty")
        .expect("an empty persisted ledger must reload");
    assert_eq!(loaded, empty);
    assert!(loaded.is_empty());
}

#[test]
fn same_millisecond_research_revoke_remains_the_latest_decision_after_reload() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let mut grant_only = ConsentLedger::new("participant_consent_reload_alpha").unwrap();
    grant_only
        .record(grant(
            "consent_event_zzz_reload_grant",
            ConsentPurpose::ResearchContribution,
            "consent_form_reload_v1",
            Some("research_scope_reload_alpha"),
            32_000,
        ))
        .unwrap();
    persist_ok(&mut client, &grant_only);

    let mut revoked = grant_only.clone();
    revoked
        .record(ConsentEventInput {
            event_ref: "consent_event_aaa_reload_revoke",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "consent_form_reload_v1",
            research_scope_ref: Some("research_scope_reload_alpha"),
            occurred_at_unix_ms: 32_000,
        })
        .unwrap();
    persist_ok(&mut client, &revoked);

    let loaded = load_ok(&mut client, "participant_consent_reload_alpha")
        .expect("persisted research consent must reload after restart");
    assert_eq!(loaded, revoked);
    let snapshot = loaded.snapshot_as("consent_snapshot_reload_alpha").unwrap();
    assert!(
        !snapshot.is_granted(ConsentPurpose::ResearchContribution),
        "same-millisecond revoke must remain the latest research decision after reload"
    );
    assert_eq!(
        snapshot.active_research_scope(),
        None,
        "a reloaded revocation must not keep the prior research scope active"
    );
}

#[test]
fn every_purpose_reloads_in_insertion_order() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let ledger = recorded_ledger(
        "participant_consent_reload_purposes",
        &[
            grant(
                "consent_event_service",
                ConsentPurpose::ServiceOperation,
                "consent_form_service",
                None,
                10_000,
            ),
            grant(
                "consent_event_account",
                ConsentPurpose::AccountPersistence,
                "consent_form_account",
                None,
                10_100,
            ),
            grant(
                "consent_event_longitudinal",
                ConsentPurpose::LongitudinalObservation,
                "consent_form_longitudinal",
                None,
                10_200,
            ),
            grant(
                "consent_event_research",
                ConsentPurpose::ResearchContribution,
                "consent_form_research",
                Some("research_scope_reload_purposes"),
                10_300,
            ),
            grant(
                "consent_event_communications",
                ConsentPurpose::Communications,
                "consent_form_communications",
                None,
                10_400,
            ),
        ],
    );
    persist_ok(&mut client, &ledger);
    let loaded = load_ok(&mut client, "participant_consent_reload_purposes")
        .expect("a multi-purpose ledger must reload");
    assert_eq!(loaded, ledger);
    let snapshot = loaded
        .snapshot_as("consent_snapshot_reload_purposes")
        .unwrap();
    assert!(snapshot.is_granted(ConsentPurpose::ServiceOperation));
    assert!(snapshot.is_granted(ConsentPurpose::AccountPersistence));
    assert!(snapshot.is_granted(ConsentPurpose::LongitudinalObservation));
    assert!(snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert!(snapshot.is_granted(ConsentPurpose::Communications));
    assert_eq!(
        snapshot.active_research_scope(),
        Some("research_scope_reload_purposes")
    );
}

#[test]
fn non_monotonic_stored_events_fail_closed_instead_of_reordering() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &ConsentLedger::new("participant_consent_reload_corrupt").unwrap(),
    );
    client
        .execute(
            "INSERT INTO consent_event (\
                 participant_ref, event_ref, consent_purpose, consent_decision, \
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
             ) VALUES ($1, $2, 'service_operation', 'granted', $3, NULL, 20_000)",
            &[
                &"participant_consent_reload_corrupt",
                &"consent_event_later",
                &"consent_form_reload_corrupt",
            ],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO consent_event (\
                 participant_ref, event_ref, consent_purpose, consent_decision, \
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
             ) VALUES ($1, $2, 'service_operation', 'revoked', $3, NULL, 19_000)",
            &[
                &"participant_consent_reload_corrupt",
                &"consent_event_earlier",
                &"consent_form_reload_corrupt",
            ],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_consent_ledger(&mut transaction, "participant_consent_reload_corrupt"),
            Err(ConsentPersistenceError::CorruptHistory)
        ),
        "out-of-order durable timestamps must not be silently reordered into a grant"
    );
    transaction.rollback().unwrap();
}

#[test]
fn equal_created_at_reload_fails_closed_instead_of_identity_order() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let mut grant_only = ConsentLedger::new("participant_consent_reload_tie").unwrap();
    grant_only
        .record(grant(
            "consent_event_zzz_reload_grant",
            ConsentPurpose::ResearchContribution,
            "consent_form_reload_tie",
            Some("research_scope_reload_tie"),
            32_000,
        ))
        .unwrap();
    persist_ok(&mut client, &grant_only);

    let mut revoked = grant_only.clone();
    revoked
        .record(ConsentEventInput {
            event_ref: "consent_event_aaa_reload_revoke",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Revoked,
            consent_form_version_ref: "consent_form_reload_tie",
            research_scope_ref: Some("research_scope_reload_tie"),
            occurred_at_unix_ms: 32_000,
        })
        .unwrap();
    persist_ok(&mut client, &revoked);

    client
        .execute(
            "UPDATE consent_event SET created_at = TIMESTAMPTZ '2026-08-16 00:00:00+00' \
             WHERE participant_ref = $1",
            &[&"participant_consent_reload_tie"],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_consent_ledger(&mut transaction, "participant_consent_reload_tie"),
            Err(ConsentPersistenceError::CorruptHistory)
        ),
        "equal created_at must not be broken by opaque event identity into a grant"
    );
    transaction.rollback().unwrap();
}

#[test]
fn consent_reload_requires_read_committed_and_rejects_blank_aliases() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_consent_ledger(&mut serializable, "participant_consent_reload_alpha"),
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut repeatable = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        load_consent_ledger(&mut repeatable, "participant_consent_reload_alpha"),
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable.rollback().unwrap();

    let mut transaction = client.transaction().unwrap();
    for invalid_ref in ["", " ", "42", " participant_consent_reload_alpha"] {
        assert!(matches!(
            load_consent_ledger(&mut transaction, invalid_ref),
            Err(ConsentPersistenceError::InvalidReference)
        ));
    }
    transaction.rollback().unwrap();
}

#[test]
fn stored_unknown_labels_and_negative_time_fail_closed_on_reload() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &recorded_ledger(
            "participant_consent_reload_labels",
            &[grant(
                "consent_event_service_label",
                ConsentPurpose::ServiceOperation,
                "consent_form_reload_label",
                None,
                11_000,
            )],
        ),
    );

    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_purpose_value_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET consent_purpose = 'unknown_purpose' \
             WHERE event_ref = 'consent_event_service_label'",
            &[],
        )
        .unwrap();
    let mut unknown_purpose = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut unknown_purpose, "participant_consent_reload_labels"),
        Err(ConsentPersistenceError::CorruptHistory)
    ));
    unknown_purpose.rollback().unwrap();

    client
        .execute(
            "UPDATE consent_event SET consent_purpose = 'service_operation' \
             WHERE event_ref = 'consent_event_service_label'",
            &[],
        )
        .unwrap();
    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_decision_value_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET consent_decision = 'unknown_decision' \
             WHERE event_ref = 'consent_event_service_label'",
            &[],
        )
        .unwrap();
    let mut unknown_decision = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut unknown_decision, "participant_consent_reload_labels"),
        Err(ConsentPersistenceError::CorruptHistory)
    ));
    unknown_decision.rollback().unwrap();

    client
        .execute(
            "UPDATE consent_event SET consent_decision = 'granted' \
             WHERE event_ref = 'consent_event_service_label'",
            &[],
        )
        .unwrap();
    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_occurred_at_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET occurred_at_unix_ms = -1 \
             WHERE event_ref = 'consent_event_service_label'",
            &[],
        )
        .unwrap();
    let mut negative_time = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut negative_time, "participant_consent_reload_labels"),
        Err(ConsentPersistenceError::InvalidTimestamp)
    ));
    negative_time.rollback().unwrap();
}

#[test]
fn missing_consent_relations_fail_closed_on_reload() {
    let _guard = consent_reload_guard();
    let mut client = test_client();
    reset_consent_tables(&mut client);
    apply_consent_migration(&mut client).unwrap();
    persist_ok(
        &mut client,
        &ConsentLedger::new("participant_consent_reload_missing").unwrap(),
    );

    client.batch_execute("DROP TABLE consent_event;").unwrap();
    let mut missing_events = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut missing_events, "participant_consent_reload_missing"),
        Err(ConsentPersistenceError::Database(_))
    ));
    missing_events.rollback().unwrap();

    client.batch_execute("DROP TABLE consent_ledger;").unwrap();
    let mut missing_ledger = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut missing_ledger, "participant_consent_reload_missing"),
        Err(ConsentPersistenceError::Database(_))
    ));
    missing_ledger.rollback().unwrap();
}
