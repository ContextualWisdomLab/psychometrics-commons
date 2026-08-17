//! Coverage regression for the `Transaction` instantiation of participant reload.
//!
//! `load_anonymous_participant_base` accepts any `PostgreSQL` client implementation. Integration
//! tests exercise the connection-backed instantiation, while this library test exercises the
//! transaction-backed instantiation without adding another production adapter or weakening the
//! exact coverage gate.

use crate::participant::ParticipantRecord;
use crate::postgres_participant::{
    load_anonymous_participant_base, persist_anonymous_participant_base,
    ParticipantBasePersistenceDisposition, ParticipantBasePersistenceError,
};
use postgres::{Client, NoTls};

#[test]
fn transaction_reload_instantiation_covers_success_absence_and_invalid_aliases() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS participant_base_library_coverage;\
             SET search_path TO participant_base_library_coverage;\
             DROP TABLE IF EXISTS assessment_participant;\
             CREATE TABLE assessment_participant (\
                 participant_ref TEXT PRIMARY KEY,\
                 tenant_ref TEXT NOT NULL,\
                 created_at_unix_ms BIGINT NOT NULL\
             );",
        )
        .unwrap();

    let participant = ParticipantRecord::new_anonymous(
        "participant_library_coverage",
        "tenant_library_coverage",
        40_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_anonymous_participant_base(&mut transaction, &participant).unwrap(),
        ParticipantBasePersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_anonymous_participant_base(&mut transaction, &participant).unwrap(),
        ParticipantBasePersistenceDisposition::Duplicate
    );

    let loaded = load_anonymous_participant_base(
        &mut transaction,
        "participant_library_coverage",
        "tenant_library_coverage",
    )
    .unwrap()
    .expect("transaction-backed reload must find its own inserted participant");
    assert_eq!(loaded.participant_ref(), "participant_library_coverage");
    assert_eq!(loaded.tenant_ref(), "tenant_library_coverage");
    assert_eq!(loaded.created_at_unix_ms(), 40_000);

    assert!(load_anonymous_participant_base(
        &mut transaction,
        "participant_library_coverage",
        "tenant_other_coverage",
    )
    .unwrap()
    .is_none());

    for (participant_ref, tenant_ref) in [
        (" participant_library_coverage ", "tenant_library_coverage"),
        ("participant_library_coverage", " tenant_library_coverage "),
    ] {
        assert!(matches!(
            load_anonymous_participant_base(&mut transaction, participant_ref, tenant_ref),
            Err(ParticipantBasePersistenceError::InvalidReference)
        ));
    }
    transaction.rollback().unwrap();
}
