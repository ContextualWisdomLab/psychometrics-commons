//! Real `PostgreSQL` persistence contract for durable integration evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::integration::{InboxDisposition, IntegrationEvent};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration, enqueue_outbox_event, PersistenceDisposition,
    PersistenceError,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(event_ref: &str, tenant_ref: &str, digest: &str) -> IntegrationEvent {
    event_from_source(
        event_ref,
        "psychometrics_commons",
        tenant_ref,
        digest,
        10_000,
    )
}

fn event_at(
    event_ref: &str,
    tenant_ref: &str,
    digest: &str,
    occurred_at_unix_ms: u64,
) -> IntegrationEvent {
    event_from_source(
        event_ref,
        "psychometrics_commons",
        tenant_ref,
        digest,
        occurred_at_unix_ms,
    )
}

fn event_from_source(
    event_ref: &str,
    source_ref: &str,
    tenant_ref: &str,
    digest: &str,
    occurred_at_unix_ms: u64,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        source_ref,
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

    let tenant_beta = event("event_alpha", "tenant_beta", DIGEST_B);
    assert_eq!(
        enqueue_outbox_event(client, &tenant_beta, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let source_beta = event_from_source(
        "event_alpha",
        "validated_partner",
        "tenant_alpha",
        DIGEST_B,
        10_000,
    );
    assert_eq!(
        enqueue_outbox_event(client, &source_beta, 3).unwrap(),
        PersistenceDisposition::Inserted
    );

    for (source_ref, tenant_ref) in [
        ("psychometrics_commons", "tenant_alpha"),
        ("psychometrics_commons", "tenant_beta"),
        ("validated_partner", "tenant_alpha"),
    ] {
        client
            .execute(
                "INSERT INTO integration_delivery_attempt (\
                     source_ref, tenant_ref, event_ref, attempt_ref, delivery_outcome,\
                     occurred_at_unix_ms\
                 ) VALUES ($1, $2, 'event_alpha', 'attempt_alpha', 'delivered', 10001)",
                &[&source_ref, &tenant_ref],
            )
            .unwrap();
    }

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
    let attempt_count: i64 = client
        .query_one("SELECT count(*) FROM integration_delivery_attempt", &[])
        .unwrap()
        .get(0);
    let inbox_count: i64 = client
        .query_one("SELECT count(*) FROM integration_inbox", &[])
        .unwrap()
        .get(0);
    assert_eq!(outbox_count, 3);
    assert_eq!(attempt_count, 3);
    assert_eq!(inbox_count, 2);
}

fn assert_outbox_reference_rejected(
    client: &mut Client,
    event_ref: &str,
    source_ref: &str,
    tenant_ref: &str,
    subject_ref: &str,
    correlation_ref: &str,
) {
    let result = client.execute(
        "INSERT INTO integration_outbox (\
             event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
             occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
             current_state, latest_event_at_unix_ms\
         ) VALUES ($1, 'assessment.session.completed', 'v1', $2, $3, $4, 1, $5, $6, 3,\
                   'pending', 1)",
        &[
            &event_ref,
            &source_ref,
            &tenant_ref,
            &subject_ref,
            &correlation_ref,
            &DIGEST_A,
        ],
    );
    assert!(result.is_err());
}

fn verify_database_constraints(client: &mut Client) {
    for invalid_reference in ["123", "-3", "+3", "1.5", "1,5", "1e5", "1E-5"] {
        assert_outbox_reference_rejected(
            client,
            invalid_reference,
            "psychometrics_commons",
            "tenant_constraint",
            "session_constraint",
            "correlation_constraint",
        );
    }
    assert_outbox_reference_rejected(
        client,
        "event_invalid_source",
        "-3",
        "tenant_constraint",
        "session_constraint",
        "correlation_constraint",
    );
    assert_outbox_reference_rejected(
        client,
        "event_invalid_tenant",
        "psychometrics_commons",
        "1.5",
        "session_constraint",
        "correlation_constraint",
    );
    assert_outbox_reference_rejected(
        client,
        "event_invalid_subject",
        "psychometrics_commons",
        "tenant_constraint",
        "1e5",
        "correlation_constraint",
    );
    assert_outbox_reference_rejected(
        client,
        "event_invalid_correlation",
        "psychometrics_commons",
        "tenant_constraint",
        "session_constraint",
        "1,5",
    );

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

fn verify_transaction_isolation_contract(client: &mut Client, tenant_alpha: &IntegrationEvent) {
    let isolation_event = event("event_isolation", "tenant_alpha", DIGEST_A);
    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        enqueue_outbox_event(&mut serializable, &isolation_event, 3),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    serializable.rollback().unwrap();

    let mut repeatable_read = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .start()
        .unwrap();
    assert!(matches!(
        accept_inbox_event(
            &mut repeatable_read,
            "consumer_isolation",
            tenant_alpha,
            16_000,
        ),
        Err(PersistenceError::UnsupportedIsolationLevel)
    ));
    repeatable_read.rollback().unwrap();
}

fn verify_persistence_error_messages() {
    assert_eq!(
        PersistenceError::InvalidReference.to_string(),
        "persistence references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters"
    );
    assert!(std::error::Error::source(&PersistenceError::InvalidReference).is_none());
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
    assert_eq!(
        PersistenceError::UnsupportedIsolationLevel.to_string(),
        "PostgreSQL integration persistence requires read committed isolation"
    );
    assert_eq!(
        PersistenceError::InvalidLeaseWindow.to_string(),
        "outbox lease expiry must be later than claim time"
    );
    assert_eq!(
        PersistenceError::InvalidFencingToken.to_string(),
        "outbox lease fencing tokens must be positive"
    );
    assert_eq!(
        PersistenceError::OutboxLeaseHeld.to_string(),
        "live outbox delivery lease rejects unfenced delivery attempts"
    );
    assert_eq!(
        PersistenceError::NotLeaseable.to_string(),
        "outbox is not currently available for an exclusive delivery lease"
    );
    assert_eq!(
        PersistenceError::NotLeased.to_string(),
        "outbox does not currently have a delivery lease"
    );
    assert_eq!(
        PersistenceError::LeaseStillActive.to_string(),
        "outbox delivery lease has not expired"
    );
    assert_eq!(
        PersistenceError::StaleLease.to_string(),
        "outbox delivery fencing token is stale"
    );
    assert_eq!(
        PersistenceError::LeaseExpired.to_string(),
        "outbox delivery lease has expired"
    );
}

fn verify_database_failures_after_dropping_schema(
    client: &mut Client,
    tenant_alpha: &IntegrationEvent,
) {
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
    assert!(std::error::Error::source(&outbox_database_error).is_some());
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
    verify_transaction_isolation_contract(&mut client, &tenant_alpha);
    verify_persistence_error_messages();
    verify_database_failures_after_dropping_schema(&mut client, &tenant_alpha);
}
