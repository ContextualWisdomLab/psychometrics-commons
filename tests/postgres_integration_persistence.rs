//! Real `PostgreSQL` persistence contract for durable integration evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxDisposition, IntegrationEvent};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration, enqueue_outbox_event, PersistenceDisposition,
    PersistenceError,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(event_ref: &str, tenant_ref: &str, digest: &str) -> IntegrationEvent {
    event_at(event_ref, tenant_ref, digest, 10_000)
}

fn event_at(
    event_ref: &str,
    tenant_ref: &str,
    digest: &str,
    occurred_at_unix_ms: u64,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        tenant_ref,
        "session_alpha",
        occurred_at_unix_ms,
        "correlation_alpha",
        None,
        digest,
    )
    .unwrap()
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn reset_integration_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_outbox;",
        )
        .unwrap();
}

fn verify_outbox_contract(client: &mut Client) -> IntegrationEvent {
    let tenant_alpha = event("event_alpha", "tenant_alpha", DIGEST_A);
    assert!(matches!(
        enqueue_outbox_event(client, &tenant_alpha, 0),
        Err(PersistenceError::InvalidAttemptLimit)
    ));
    assert!(matches!(
        enqueue_outbox_event(client, &tenant_alpha, i32::MAX as usize + 1),
        Err(PersistenceError::ValueOutOfRange)
    ));
    let out_of_range_event = event_at("event_range", "tenant_alpha", DIGEST_A, u64::MAX);
    assert!(matches!(
        enqueue_outbox_event(client, &out_of_range_event, 3),
        Err(PersistenceError::ValueOutOfRange)
    ));

    assert_eq!(
        enqueue_outbox_event(client, &tenant_alpha, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    assert_eq!(
        enqueue_outbox_event(client, &tenant_alpha, 3).unwrap(),
        PersistenceDisposition::Duplicate
    );

    let conflicting = event("event_alpha", "tenant_alpha", DIGEST_B);
    assert!(matches!(
        enqueue_outbox_event(client, &conflicting, 3),
        Err(PersistenceError::ConflictingReplay)
    ));
    assert!(matches!(
        enqueue_outbox_event(client, &tenant_alpha, 4),
        Err(PersistenceError::ConflictingReplay)
    ));

    let transactional_event = event("event_transaction", "tenant_alpha", DIGEST_A);
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            enqueue_outbox_event(&mut transaction, &transactional_event, 3).unwrap(),
            PersistenceDisposition::Inserted
        );
        transaction.rollback().unwrap();
    }
    let rolled_back_count: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_outbox WHERE event_ref = 'event_transaction'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(rolled_back_count, 0);

    tenant_alpha
}

fn verify_inbox_contract(client: &mut Client, tenant_alpha: &IntegrationEvent) {
    assert!(matches!(
        accept_inbox_event(client, "123", tenant_alpha, 11_000),
        Err(PersistenceError::InvalidReference)
    ));
    assert!(matches!(
        accept_inbox_event(client, "consumer_alpha", tenant_alpha, 0),
        Err(PersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        accept_inbox_event(client, "consumer_alpha", tenant_alpha, u64::MAX),
        Err(PersistenceError::ValueOutOfRange)
    ));

    assert_eq!(
        accept_inbox_event(client, "consumer_alpha", tenant_alpha, 11_000).unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(
        accept_inbox_event(client, "consumer_alpha", tenant_alpha, 12_000).unwrap(),
        InboxDisposition::Duplicate
    );

    let tenant_beta = event("event_alpha", "tenant_beta", DIGEST_B);
    assert_eq!(
        accept_inbox_event(client, "consumer_alpha", &tenant_beta, 13_000).unwrap(),
        InboxDisposition::Accepted
    );

    let conflicting_inbox = event("event_alpha", "tenant_alpha", DIGEST_B);
    assert!(matches!(
        accept_inbox_event(client, "consumer_alpha", &conflicting_inbox, 14_000),
        Err(PersistenceError::ConflictingReplay)
    ));
}

fn verify_persisted_row_counts(client: &mut Client) {
    let outbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    let inbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_inbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox_count, 1);
    assert_eq!(inbox_count, 2);
}

fn verify_database_constraints(client: &mut Client) {
    let invalid_reference = client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
             occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
             current_state, latest_event_at_unix_ms\
         ) VALUES ('123', 'assessment.session.completed', 'v1', 'psychometrics_commons',\
                   'tenant_alpha', 'session_alpha', 1, 'correlation_alpha', $1, 3, 'pending', 1)",
        &[&DIGEST_A],
    );
    assert!(invalid_reference.is_err());

    let invalid_digest = client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
             occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
             current_state, latest_event_at_unix_ms\
         ) VALUES ('event_invalid_digest', 'assessment.session.completed', 'v1',\
                   'psychometrics_commons', 'tenant_alpha', 'session_alpha', 1,\
                   'correlation_alpha', 'sha256:not-a-digest', 3, 'pending', 1)",
        &[],
    );
    assert!(invalid_digest.is_err());
}

fn verify_persistence_error_messages() {
    assert_eq!(
        PersistenceError::InvalidReference.to_string(),
        "persistence references must be opaque non-numeric values"
    );
    assert_eq!(
        PersistenceError::InvalidTimestamp.to_string(),
        "persistence timestamps must be greater than zero"
    );
    assert_eq!(
        PersistenceError::InvalidAttemptLimit.to_string(),
        "outbox maximum attempts must be greater than zero"
    );
    assert_eq!(
        PersistenceError::ValueOutOfRange.to_string(),
        "persistence value exceeds the supported PostgreSQL range"
    );
    assert_eq!(
        PersistenceError::ConflictingReplay.to_string(),
        "persistence idempotency identity was replayed with conflicting evidence"
    );
}

fn verify_database_failures(client: &mut Client, tenant_alpha: &IntegrationEvent) {
    client
        .batch_execute(
            "DROP TABLE integration_inbox;\
             DROP TABLE integration_delivery_attempt;\
             DROP TABLE integration_outbox;",
        )
        .unwrap();
    let outbox_database_error = enqueue_outbox_event(client, tenant_alpha, 3).unwrap_err();
    assert!(matches!(
        outbox_database_error,
        PersistenceError::Database(_)
    ));
    assert_eq!(
        outbox_database_error.to_string(),
        "PostgreSQL persistence operation failed"
    );
    assert!(matches!(
        accept_inbox_event(client, "consumer_alpha", tenant_alpha, 15_000),
        Err(PersistenceError::Database(_))
    ));
}

#[test]
fn integration_evidence_persists_with_exact_replay_and_tenant_isolation() {
    let mut client = test_client();
    reset_integration_tables(&mut client);
    apply_integration_migration(&mut client).unwrap();
    apply_integration_migration(&mut client).unwrap();

    let tenant_alpha = verify_outbox_contract(&mut client);
    verify_inbox_contract(&mut client, &tenant_alpha);
    verify_persisted_row_counts(&mut client);
    verify_database_constraints(&mut client);
    verify_persistence_error_messages();
    verify_database_failures(&mut client, &tenant_alpha);
}
