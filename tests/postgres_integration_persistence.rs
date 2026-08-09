//! Real PostgreSQL persistence contract for durable integration evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxDisposition, IntegrationEvent};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration, enqueue_outbox_event, PersistenceDisposition,
    PersistenceError,
};

const DIGEST_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(event_ref: &str, tenant_ref: &str, digest: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        tenant_ref,
        "session_alpha",
        10_000,
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

#[test]
fn integration_evidence_persists_with_exact_replay_and_tenant_isolation() {
    let mut client = test_client();
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_outbox;",
        )
        .unwrap();

    apply_integration_migration(&mut client).unwrap();
    apply_integration_migration(&mut client).unwrap();

    let tenant_alpha = event("event_alpha", "tenant_alpha", DIGEST_A);
    assert_eq!(
        enqueue_outbox_event(&mut client, &tenant_alpha, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    assert_eq!(
        enqueue_outbox_event(&mut client, &tenant_alpha, 3).unwrap(),
        PersistenceDisposition::Duplicate
    );

    let conflicting = event("event_alpha", "tenant_alpha", DIGEST_B);
    assert!(matches!(
        enqueue_outbox_event(&mut client, &conflicting, 3),
        Err(PersistenceError::ConflictingReplay)
    ));

    assert_eq!(
        accept_inbox_event(&mut client, "consumer_alpha", &tenant_alpha, 11_000).unwrap(),
        InboxDisposition::Accepted
    );
    assert_eq!(
        accept_inbox_event(&mut client, "consumer_alpha", &tenant_alpha, 12_000).unwrap(),
        InboxDisposition::Duplicate
    );

    let tenant_beta = event("event_alpha", "tenant_beta", DIGEST_B);
    assert_eq!(
        accept_inbox_event(&mut client, "consumer_alpha", &tenant_beta, 13_000).unwrap(),
        InboxDisposition::Accepted
    );

    let conflicting_inbox = event("event_alpha", "tenant_alpha", DIGEST_B);
    assert!(matches!(
        accept_inbox_event(&mut client, "consumer_alpha", &conflicting_inbox, 14_000),
        Err(PersistenceError::ConflictingReplay)
    ));

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
}
