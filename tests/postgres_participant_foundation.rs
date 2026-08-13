//! Real PostgreSQL contract for tenant-owned stable participant persistence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_foundation_migration, persist_participant, register_tenant,
    ParticipantPersistenceDisposition, ParticipantPersistenceError,
};
use std::sync::{Mutex, MutexGuard};

static PARTICIPANT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn participant_test_guard() -> MutexGuard<'static, ()> {
    PARTICIPANT_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS participant_foundation_test;\
             SET search_path TO participant_foundation_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS participant_foundation_test.assessment_participant;\
             DROP TABLE IF EXISTS participant_foundation_test.tenant_account;",
        )
        .unwrap();
}

#[test]
fn tenant_and_participant_registration_are_exactly_idempotent() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_foundation_migration(&mut client).unwrap();

    assert_eq!(
        register_tenant(&mut client, "tenant_alpha").unwrap(),
        ParticipantPersistenceDisposition::Inserted
    );
    assert_eq!(
        register_tenant(&mut client, "tenant_alpha").unwrap(),
        ParticipantPersistenceDisposition::Duplicate
    );

    let participant = ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1_000)
        .unwrap();
    assert_eq!(
        persist_participant(&mut client, &participant).unwrap(),
        ParticipantPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_participant(&mut client, &participant).unwrap(),
        ParticipantPersistenceDisposition::Duplicate
    );
}

#[test]
fn participant_identity_cannot_be_rebound_to_another_tenant_or_creation_time() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_foundation_migration(&mut client).unwrap();
    register_tenant(&mut client, "tenant_alpha").unwrap();
    register_tenant(&mut client, "tenant_beta").unwrap();

    let original = ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1_000)
        .unwrap();
    persist_participant(&mut client, &original).unwrap();

    let tenant_rebind = ParticipantRecord::new_anonymous("participant_alpha", "tenant_beta", 1_000)
        .unwrap();
    assert!(matches!(
        persist_participant(&mut client, &tenant_rebind),
        Err(ParticipantPersistenceError::ConflictingReplay)
    ));

    let time_rebind = ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 2_000)
        .unwrap();
    assert!(matches!(
        persist_participant(&mut client, &time_rebind),
        Err(ParticipantPersistenceError::ConflictingReplay)
    ));
}

#[test]
fn participant_requires_a_registered_tenant_and_database_fk_enforces_ownership() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_foundation_migration(&mut client).unwrap();

    let orphan = ParticipantRecord::new_anonymous("participant_orphan", "tenant_missing", 1_000)
        .unwrap();
    assert!(matches!(
        persist_participant(&mut client, &orphan),
        Err(ParticipantPersistenceError::TenantNotFound)
    ));

    let raw_orphan = client.execute(
        "INSERT INTO assessment_participant (participant_ref, tenant_ref, created_at_unix_ms) \
         VALUES ('participant_raw', 'tenant_missing', 1000)",
        &[],
    );
    assert!(raw_orphan.is_err());
}

#[test]
fn tenant_deletion_is_restricted_while_owned_participants_exist() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_foundation_migration(&mut client).unwrap();
    register_tenant(&mut client, "tenant_alpha").unwrap();
    let participant = ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 1_000)
        .unwrap();
    persist_participant(&mut client, &participant).unwrap();

    assert!(client
        .execute("DELETE FROM tenant_account WHERE tenant_ref = 'tenant_alpha'", &[])
        .is_err());
}

#[test]
fn tenant_registration_rejects_nonopaque_references() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_participant_foundation_migration(&mut client).unwrap();

    for invalid in ["", "   ", "123", "1e3"] {
        assert!(matches!(
            register_tenant(&mut client, invalid),
            Err(ParticipantPersistenceError::InvalidReference)
        ));
    }
}
