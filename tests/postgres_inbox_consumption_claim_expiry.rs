//! Regression coverage for terminal transitions after an inbox claim expires.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, complete_inbox_consumption,
    persist_inbox_consumption, quarantine_inbox_consumption,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};
use std::time::{SystemTime, UNIX_EPOCH};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn isolated_client() -> (Client, String) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    let schema_name = format!(
        "inbox_claim_expiry_{}_{}",
        std::process::id(),
        nonce
    );
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();
    (client, schema_name)
}

fn source_event(event_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "assessment.session.completed",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        None,
        DIGEST,
    )
    .unwrap()
}

fn pending(event_ref: &str, consumption_ref: &str) -> InboxConsumption {
    InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        event_ref,
        consumption_ref,
        "side_effect_projection",
        20_000,
    )
    .unwrap()
}

fn prepare_claim(client: &mut Client, event_ref: &str, consumption_ref: &str) -> InboxConsumption {
    accept_inbox_event(client, "consumer_alpha", &source_event(event_ref), 20_000).unwrap();
    let consumption = pending(event_ref, consumption_ref);
    let mut transaction = client.transaction().unwrap();
    persist_inbox_consumption(&mut transaction, &consumption).unwrap();
    let fence = begin_inbox_consumption(&mut transaction, &consumption, 20_001, 21_000).unwrap();
    assert_eq!(fence, 1);
    transaction.commit().unwrap();
    consumption
}

fn assert_processing(client: &mut Client, consumption_ref: &str) {
    let row = client
        .query_one(
            "SELECT consumption_state, fencing_token, claim_expires_at_unix_ms \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption_ref],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "processing");
    assert_eq!(row.get::<_, i64>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(21_000));
}

#[test]
fn expired_claim_cannot_complete_at_or_after_expiry_with_current_fence() {
    let (mut client, schema_name) = isolated_client();
    let consumption = prepare_claim(
        &mut client,
        "event_expired_complete",
        "consumption_expired_complete",
    );

    for observed_at_unix_ms in [21_000, 21_001] {
        let mut transaction = client.transaction().unwrap();
        assert!(
            complete_inbox_consumption(
                &mut transaction,
                &consumption,
                observed_at_unix_ms,
                "completion_after_expiry",
                1,
            )
            .is_err(),
            "the current fence must not authorize completion after claim expiry"
        );
        transaction.rollback().unwrap();
        assert_processing(&mut client, consumption.consumption_ref());
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}

#[test]
fn expired_claim_cannot_quarantine_at_or_after_expiry_with_current_fence() {
    let (mut client, schema_name) = isolated_client();
    let consumption = prepare_claim(
        &mut client,
        "event_expired_quarantine",
        "consumption_expired_quarantine",
    );

    for observed_at_unix_ms in [21_000, 21_001] {
        let mut transaction = client.transaction().unwrap();
        assert!(
            quarantine_inbox_consumption(
                &mut transaction,
                &consumption,
                observed_at_unix_ms,
                "poison_payload",
                1,
            )
            .is_err(),
            "the current fence must not authorize quarantine after claim expiry"
        );
        transaction.rollback().unwrap();
        assert_processing(&mut client, consumption.consumption_ref());
    }

    client
        .batch_execute(&format!("DROP SCHEMA {schema_name} CASCADE;"))
        .unwrap();
}
