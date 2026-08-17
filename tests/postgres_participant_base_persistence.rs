//! Real `PostgreSQL` contract for the durable anonymous-first participant base record.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_base_migration, load_anonymous_participant_base,
    persist_anonymous_participant_base, ParticipantBasePersistenceDisposition,
    ParticipantBasePersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static PARTICIPANT_BASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn participant_base_test_guard() -> MutexGuard<'static, ()> {
    PARTICIPANT_BASE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS participant_base_persistence_test;\
             SET search_path TO participant_base_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_participant_base_table(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS participant_base_persistence_test.assessment_participant;",
        )
        .unwrap();
}

fn anonymous_participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous("participant_public_demo", "tenant_public_demo", 40_000)
        .unwrap()
}

#[test]
fn anonymous_base_round_trip_is_exact_and_tenant_bound() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    let participant = anonymous_participant();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_participant_base(&mut transaction, &participant).unwrap(),
            ParticipantBasePersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_participant_base(&mut transaction, &participant).unwrap(),
            ParticipantBasePersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let loaded = load_anonymous_participant_base(
        &mut client,
        "participant_public_demo",
        "tenant_public_demo",
    )
    .unwrap()
    .expect("stored participant must reload");
    assert_eq!(loaded.participant_ref(), participant.participant_ref());
    assert_eq!(loaded.tenant_ref(), participant.tenant_ref());
    assert_eq!(
        loaded.created_at_unix_ms(),
        participant.created_at_unix_ms()
    );
    assert!(loaded.link_history().is_empty());
    assert!(loaded.link_end_history().is_empty());

    assert!(load_anonymous_participant_base(
        &mut client,
        "participant_public_demo",
        "tenant_other_demo",
    )
    .unwrap()
    .is_none());
}

#[test]
fn participant_identity_rebinding_fails_closed_without_rewriting_the_row() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    let participant = anonymous_participant();
    {
        let mut transaction = client.transaction().unwrap();
        persist_anonymous_participant_base(&mut transaction, &participant).unwrap();
        transaction.commit().unwrap();
    }

    let rebound =
        ParticipantRecord::new_anonymous("participant_public_demo", "tenant_rebound_demo", 40_001)
            .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_anonymous_participant_base(&mut transaction, &rebound),
        Err(ParticipantBasePersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT tenant_ref, created_at_unix_ms FROM assessment_participant \
             WHERE participant_ref = 'participant_public_demo'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "tenant_public_demo");
    assert_eq!(row.get::<_, i64>(1), 40_000);
}

#[test]
fn linked_records_cannot_be_misrepresented_as_complete_base_only_state() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    let mut participant = anonymous_participant();
    participant
        .link_account(
            "link_event_demo",
            "issuer_keyverse_demo",
            "subject_keyverse_demo",
            "proof_anonymous_demo",
            "proof_authenticated_demo",
            40_100,
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_anonymous_participant_base(&mut transaction, &participant),
        Err(ParticipantBasePersistenceError::LinkedRecordRequiresIdentityHistory)
    ));
    transaction.rollback().unwrap();

    let count: i64 = client
        .query_one("SELECT COUNT(*) FROM assessment_participant", &[])
        .unwrap()
        .get(0);
    assert_eq!(count, 0);
}

#[test]
fn stronger_isolation_is_rejected_before_insert() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    let participant = anonymous_participant();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        persist_anonymous_participant_base(&mut transaction, &participant),
        Err(ParticipantBasePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_participant_relation_is_a_database_failure() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);

    let participant = anonymous_participant();
    let mut transaction = client.transaction().unwrap();
    let persist_error = persist_anonymous_participant_base(&mut transaction, &participant)
        .expect_err("persist must fail closed when the participant relation is missing");
    transaction.rollback().unwrap();
    assert!(
        matches!(
            persist_error,
            ParticipantBasePersistenceError::Database(_)
        ),
        "missing relation must be a database failure, not a reconstructed identity: {persist_error}"
    );
    assert_eq!(
        persist_error.to_string(),
        "PostgreSQL participant base persistence failed"
    );
    assert!(std::error::Error::source(&persist_error).is_some());

    let load_error = load_anonymous_participant_base(
        &mut client,
        "participant_public_demo",
        "tenant_public_demo",
    )
    .expect_err("load must fail closed when the participant relation is missing");
    assert!(
        matches!(load_error, ParticipantBasePersistenceError::Database(_)),
        "missing relation must be a database failure, not absence: {load_error}"
    );
}

#[test]
fn schema_rejects_blank_numeric_and_nonpositive_identity_evidence() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    for statement in [
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ('12', 'tenant_public_demo', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ('participant_public_demo', ' ', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ('participant_public_demo', 'tenant_public_demo', 0)",
    ] {
        assert!(client.batch_execute(statement).is_err());
    }
}

#[test]
fn schema_rejects_unicode_reference_forms_rejected_by_the_domain_contract() {
    let _guard = participant_base_test_guard();
    let mut client = test_client();
    reset_participant_base_table(&mut client);
    apply_participant_base_migration(&mut client).unwrap();

    for statement in [
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES (E'\\tparticipant_public_demo', 'tenant_public_demo', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ('participant_public_demo', U&'\\00A0tenant_public_demo', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES (U&'12\\066B3', 'tenant_public_demo', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES (U&'12\\FF0E3', 'tenant_public_demo', 40000)",
        "INSERT INTO assessment_participant \
         (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES (U&'\\0661\\0662\\066B\\0663', 'tenant_public_demo', 40000)",
    ] {
        assert!(
            client.batch_execute(statement).is_err(),
            "database constraint must reject identity spelling the Rust domain would reject: {statement}"
        );
    }

    let count: i64 = client
        .query_one("SELECT COUNT(*) FROM assessment_participant", &[])
        .unwrap()
        .get(0);
    assert_eq!(
        count, 0,
        "invalid direct SQL must leave no corrupt identity row"
    );
    assert!(load_anonymous_participant_base(
        &mut client,
        "participant_public_demo",
        "tenant_public_demo",
    )
    .unwrap()
    .is_none());
}
