//! Real PostgreSQL recovery acceptance for the anonymous participant base record.
//!
//! The participant identity is recovery-critical product state: a restore must preserve the exact
//! participant/tenant binding and creation time, and must not reopen that identifier for rebinding.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_participant_base_migration, load_anonymous_participant_base,
    persist_anonymous_participant_base, ParticipantBasePersistenceDisposition,
    ParticipantBasePersistenceError,
};
use std::io::{Read, Write};

const SOURCE_SCHEMA: &str = "participant_base_recovery_source_test";
const RESTORED_SCHEMA: &str = "participant_base_recovery_restored_test";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn recreate_schema(client: &mut Client, schema: &str) {
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("participant recovery schema should be recreated");
    apply_participant_base_migration(client)
        .expect("participant base migration should apply to recovery schema");
}

fn copy_participant_out(client: &mut Client) -> Vec<u8> {
    let mut reader = client
        .copy_out(&format!(
            "COPY {SOURCE_SCHEMA}.assessment_participant TO STDOUT (FORMAT BINARY)"
        ))
        .expect("participant backup stream should open");
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .expect("participant backup stream should be readable");
    assert!(
        !bytes.is_empty(),
        "participant backup stream must contain PostgreSQL binary COPY data"
    );
    bytes
}

fn copy_participant_in(client: &mut Client, bytes: &[u8]) {
    let mut writer = client
        .copy_in(&format!(
            "COPY {RESTORED_SCHEMA}.assessment_participant FROM STDIN (FORMAT BINARY)"
        ))
        .expect("participant restore stream should open");
    writer
        .write_all(bytes)
        .expect("participant restore stream should accept backup bytes");
    writer
        .finish()
        .expect("participant restore stream should commit");
}

#[test]
fn restore_preserves_exact_participant_identity_and_rebinding_guard() {
    let mut client = test_client();
    recreate_schema(&mut client, SOURCE_SCHEMA);

    let participant = ParticipantRecord::new_anonymous(
        "participant_recovery_demo",
        "tenant_recovery_demo",
        40_000,
    )
    .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_anonymous_participant_base(&mut transaction, &participant).unwrap(),
            ParticipantBasePersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let backup = copy_participant_out(&mut client);
    recreate_schema(&mut client, RESTORED_SCHEMA);
    copy_participant_in(&mut client, &backup);
    client
        .batch_execute(&format!("SET search_path TO {RESTORED_SCHEMA};"))
        .unwrap();

    let restored = load_anonymous_participant_base(
        &mut client,
        "participant_recovery_demo",
        "tenant_recovery_demo",
    )
    .unwrap()
    .expect("restored participant must remain available to its owning tenant");
    assert_eq!(restored.participant_ref(), "participant_recovery_demo");
    assert_eq!(restored.tenant_ref(), "tenant_recovery_demo");
    assert_eq!(restored.created_at_unix_ms(), 40_000);

    let rebound = ParticipantRecord::new_anonymous(
        "participant_recovery_demo",
        "tenant_recovery_other",
        40_001,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_anonymous_participant_base(&mut transaction, &rebound),
        Err(ParticipantBasePersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    assert!(load_anonymous_participant_base(
        &mut client,
        "participant_recovery_demo",
        "tenant_recovery_other",
    )
    .unwrap()
    .is_none());

    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SOURCE_SCHEMA} CASCADE; DROP SCHEMA IF EXISTS {RESTORED_SCHEMA} CASCADE;"
        ))
        .expect("participant recovery schemas should be removed");
}
