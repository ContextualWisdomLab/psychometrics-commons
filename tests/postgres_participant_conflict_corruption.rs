//! Corrupt replay-winner evidence must not be misclassified as a legitimate conflict.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    persist_anonymous_participant_base, ParticipantBasePersistenceError,
};

fn test_client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable")
}

#[test]
fn corrupt_conflict_winner_is_reported_as_corrupt_stored_identity() {
    let participant = ParticipantRecord::new_anonymous(
        "participant_conflict_corruption",
        "tenant_conflict_corruption",
        40_000,
    )
    .unwrap();
    let mut client = test_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS participant_conflict_corruption_test;\
             SET search_path TO participant_conflict_corruption_test;\
             DROP TABLE IF EXISTS assessment_participant;\
             CREATE TABLE assessment_participant (\
                 participant_ref TEXT PRIMARY KEY,\
                 tenant_ref TEXT NOT NULL,\
                 created_at_unix_ms BIGINT NOT NULL\
             );",
        )
        .unwrap();

    for (stored_tenant_ref, stored_created_at_unix_ms) in [
        (" ", 40_000_i64),
        ("tenant_conflict_corruption", 0_i64),
    ] {
        client
            .execute(
                "INSERT INTO assessment_participant \
                 (participant_ref, tenant_ref, created_at_unix_ms) VALUES ($1, $2, $3)",
                &[
                    &participant.participant_ref(),
                    &stored_tenant_ref,
                    &stored_created_at_unix_ms,
                ],
            )
            .unwrap();

        let mut transaction = client.transaction().unwrap();
        let error = persist_anonymous_participant_base(&mut transaction, &participant)
            .expect_err("corrupt stored winner evidence must fail closed before replay classification");
        assert!(matches!(
            error,
            ParticipantBasePersistenceError::CorruptStoredIdentity
        ));
        transaction.rollback().unwrap();

        client
            .execute(
                "DELETE FROM assessment_participant WHERE participant_ref = $1",
                &[&participant.participant_ref()],
            )
            .unwrap();
    }
}
