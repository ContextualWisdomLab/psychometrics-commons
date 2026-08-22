//! Real PostgreSQL regression contract for restart-safe consent reconstruction.
//!
//! Purpose-specific consent is append-only evidence. Reload must preserve the
//! persisted event order for same-millisecond decisions and fail closed when
//! stored history cannot be reconstructed without guessing an order.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent::{
    apply_consent_migration, load_consent_ledger, persist_consent_ledger, ConsentPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const LEGACY_CONSENT_MIGRATION: &str = include_str!("../migrations/0005_consent_lifecycle.sql");

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

fn drop_consent_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS consent_reload_regression_test.consent_event;\
             DROP TABLE IF EXISTS consent_reload_regression_test.consent_ledger;",
        )
        .unwrap();
}

fn reset(client: &mut Client) {
    drop_consent_tables(client);
    apply_consent_migration(client).unwrap();
}

fn reset_legacy_schema(client: &mut Client) {
    drop_consent_tables(client);
    client.batch_execute(LEGACY_CONSENT_MIGRATION).unwrap();
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

fn insert_legacy_research_event(
    client: &mut Client,
    participant_ref: &str,
    event_ref: &str,
    decision: &str,
    occurred_at_unix_ms: i64,
) {
    client
        .execute(
            "INSERT INTO consent_event (\
                 participant_ref, event_ref, consent_purpose, consent_decision, \
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
             ) VALUES ($1, $2, 'research_contribution', $3, $4, $5, $6)",
            &[
                &participant_ref,
                &event_ref,
                &decision,
                &"consent_form_research_v1",
                &"research_scope_research_v1",
                &occurred_at_unix_ms,
            ],
        )
        .unwrap();
}

#[test]
fn missing_and_empty_ledgers_do_not_invent_consent() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    assert!(load(&mut client, "participant_consent_reload_missing").is_none());

    let empty = ConsentLedger::new("participant_consent_reload_empty").unwrap();
    persist(&mut client, &empty);
    let loaded = load(&mut client, "participant_consent_reload_empty")
        .expect("an explicitly persisted empty ledger must reload");
    assert_eq!(loaded, empty);
    assert!(loaded.is_empty());
}

#[test]
fn one_persist_call_with_multiple_events_reloads_exactly() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut ledger = ConsentLedger::new("participant_consent_reload_batch").unwrap();
    ledger
        .record(research_event(
            "consent_event_batch_grant",
            ConsentDecision::Granted,
            30_000,
        ))
        .unwrap();
    ledger
        .record(research_event(
            "consent_event_batch_revoke",
            ConsentDecision::Revoked,
            30_000,
        ))
        .unwrap();

    persist(&mut client, &ledger);

    client
        .execute(
            "UPDATE consent_event \
             SET created_at = TIMESTAMPTZ '2026-08-21 00:00:00+00' \
             WHERE participant_ref = $1",
            &[&"participant_consent_reload_batch"],
        )
        .unwrap();

    let loaded = load(&mut client, "participant_consent_reload_batch")
        .expect("a single multi-event persist must remain restart-reconstructable");
    let snapshot = loaded.snapshot_as("consent_snapshot_reload_batch").unwrap();
    assert_eq!(loaded, ledger);
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
}

#[test]
fn same_millisecond_revoke_remains_latest_after_restart_and_sequence_is_contiguous() {
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

    let stored_order: Vec<(String, Option<i64>)> = client
        .query(
            "SELECT event_ref, event_sequence FROM consent_event \
             WHERE participant_ref = $1 ORDER BY event_sequence",
            &[&"participant_consent_reload_alpha"],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        stored_order,
        vec![
            ("consent_event_zzz_grant".to_owned(), Some(1)),
            ("consent_event_aaa_revoke".to_owned(), Some(2)),
        ]
    );
}

#[test]
fn wall_clock_rollback_cannot_reverse_same_millisecond_consent_order() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut grant_only = ConsentLedger::new("participant_consent_reload_clock").unwrap();
    grant_only
        .record(research_event(
            "consent_event_clock_grant",
            ConsentDecision::Granted,
            35_000,
        ))
        .unwrap();
    persist(&mut client, &grant_only);

    let mut revoked = grant_only.clone();
    revoked
        .record(research_event(
            "consent_event_clock_revoke",
            ConsentDecision::Revoked,
            35_000,
        ))
        .unwrap();
    persist(&mut client, &revoked);

    client
        .execute(
            "UPDATE consent_event \
             SET created_at = CASE event_ref \
                 WHEN 'consent_event_clock_grant' THEN TIMESTAMPTZ '2026-08-21 00:00:02+00' \
                 ELSE TIMESTAMPTZ '2026-08-21 00:00:01+00' END \
             WHERE participant_ref = $1",
            &[&"participant_consent_reload_clock"],
        )
        .unwrap();

    let loaded = load(&mut client, "participant_consent_reload_clock")
        .expect("wall-clock movement must not destroy durable event order");
    let snapshot = loaded.snapshot_as("consent_snapshot_reload_clock").unwrap();
    assert_eq!(loaded, revoked);
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
}

#[test]
fn created_at_ties_do_not_override_sequence_and_noncanonical_aliases_fail_closed() {
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

    let loaded = load(&mut client, "participant_consent_reload_tie")
        .expect("created_at is evidence metadata, not the ledger order authority");
    assert_eq!(loaded, revoked);

    let mut alias_transaction = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut alias_transaction, " participant_consent_reload_tie"),
        Err(ConsentPersistenceError::InvalidReference)
    ));
    alias_transaction.rollback().unwrap();
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
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms, event_sequence\
             ) VALUES ($1, $2, 'service_operation', 'granted', $3, NULL, 20000, 1)",
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
                 consent_form_version_ref, research_scope_ref, occurred_at_unix_ms, event_sequence\
             ) VALUES ($1, $2, 'service_operation', 'revoked', $3, NULL, 19000, 2)",
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
fn gapped_sequence_fails_closed() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut ledger = ConsentLedger::new("participant_consent_reload_gap").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_gap_one",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_gap_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 50_000,
        })
        .unwrap();
    persist(&mut client, &ledger);
    client
        .execute(
            "UPDATE consent_event SET event_sequence = 2 WHERE participant_ref = $1",
            &[&"participant_consent_reload_gap"],
        )
        .unwrap();
    let mut gapped = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut gapped, "participant_consent_reload_gap"),
        Err(ConsentPersistenceError::CorruptHistory)
    ));
    gapped.rollback().unwrap();
}

#[test]
fn single_legacy_event_upgrades_without_inventing_history_order() {
    let _guard = guard();
    let mut client = test_client();
    reset_legacy_schema(&mut client);

    client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
            &[&"participant_consent_reload_legacy_one"],
        )
        .unwrap();
    insert_legacy_research_event(
        &mut client,
        "participant_consent_reload_legacy_one",
        "consent_event_legacy_one_grant",
        "granted",
        52_000,
    );
    apply_consent_migration(&mut client).unwrap();

    let legacy = load(&mut client, "participant_consent_reload_legacy_one")
        .expect("one historical event has no relative-order ambiguity");
    let legacy_snapshot = legacy.snapshot_as("consent_snapshot_legacy_one").unwrap();
    assert!(legacy_snapshot.is_granted(ConsentPurpose::ResearchContribution));

    let mut extended = legacy.clone();
    extended
        .record(research_event(
            "consent_event_legacy_one_revoke",
            ConsentDecision::Revoked,
            52_000,
        ))
        .unwrap();
    persist(&mut client, &extended);

    let reloaded = load(&mut client, "participant_consent_reload_legacy_one")
        .expect("a sequenced tail may extend one unambiguous legacy event");
    let snapshot = reloaded
        .snapshot_as("consent_snapshot_legacy_extended")
        .unwrap();
    assert_eq!(reloaded, extended);
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));

    let stored_order: Vec<(String, Option<i64>)> = client
        .query(
            "SELECT event_ref, event_sequence FROM consent_event \
             WHERE participant_ref = $1 ORDER BY event_sequence ASC NULLS FIRST",
            &[&"participant_consent_reload_legacy_one"],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        stored_order,
        vec![
            ("consent_event_legacy_one_grant".to_owned(), None),
            ("consent_event_legacy_one_revoke".to_owned(), Some(1)),
        ]
    );
}

#[test]
fn stored_label_time_and_relation_corruption_propagate_as_typed_failures() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut ledger = ConsentLedger::new("participant_consent_reload_labels").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_service_label",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_service_label",
            research_scope_ref: None,
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    persist(&mut client, &ledger);

    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_purpose_value_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET consent_purpose = 'unknown_purpose' WHERE event_ref = $1",
            &[&"consent_event_service_label"],
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
            "UPDATE consent_event SET consent_purpose = 'service_operation' WHERE event_ref = $1",
            &[&"consent_event_service_label"],
        )
        .unwrap();
    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_decision_value_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET consent_decision = 'unknown_decision' WHERE event_ref = $1",
            &[&"consent_event_service_label"],
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
            "UPDATE consent_event SET consent_decision = 'granted' WHERE event_ref = $1",
            &[&"consent_event_service_label"],
        )
        .unwrap();
    client
        .batch_execute(
            "ALTER TABLE consent_event DROP CONSTRAINT consent_event_occurred_at_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "UPDATE consent_event SET occurred_at_unix_ms = -1 WHERE event_ref = $1",
            &[&"consent_event_service_label"],
        )
        .unwrap();
    let mut negative_time = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut negative_time, "participant_consent_reload_labels"),
        Err(ConsentPersistenceError::InvalidTimestamp)
    ));
    negative_time.rollback().unwrap();

    reset(&mut client);
    persist(
        &mut client,
        &ConsentLedger::new("participant_consent_reload_relation").unwrap(),
    );
    client.batch_execute("DROP TABLE consent_event;").unwrap();
    let mut missing_events = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut missing_events, "participant_consent_reload_relation"),
        Err(ConsentPersistenceError::Database(_))
    ));
    missing_events.rollback().unwrap();

    client.batch_execute("DROP TABLE consent_ledger;").unwrap();
    let mut missing_header = client.transaction().unwrap();
    assert!(matches!(
        load_consent_ledger(&mut missing_header, "participant_consent_reload_relation"),
        Err(ConsentPersistenceError::Database(_))
    ));
    missing_header.rollback().unwrap();
    reset(&mut client);
}

#[test]
fn reload_requires_read_committed() {
    let _guard = guard();
    let mut client = test_client();
    reset(&mut client);

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_consent_ledger(&mut serializable, "participant_consent_reload_isolation"),
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut repeatable = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        load_consent_ledger(&mut repeatable, "participant_consent_reload_isolation"),
        Err(ConsentPersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable.rollback().unwrap();
}
