//! Real `PostgreSQL` contract that successful terminal inbox writes clear lease evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, complete_inbox_consumption,
    persist_inbox_consumption, quarantine_inbox_consumption, InboxConsumptionDisposition,
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
    let schema = format!("inbox_terminal_deadline_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated inbox terminal-deadline schema should be created");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_inbox_consumption_migration(&mut client)
        .expect("inbox-consumption migrations should apply atomically");
    (client, schema)
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

fn claimed_consumption(
    client: &mut Client,
    event_ref: &str,
    consumption_ref: &str,
) -> InboxConsumption {
    accept_inbox_event(client, "consumer_alpha", &source_event(event_ref), 20_000).unwrap();
    let consumption = InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        event_ref,
        consumption_ref,
        "side_effect_projection",
        20_000,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_inbox_consumption(&mut transaction, &consumption).unwrap();
    assert_eq!(
        begin_inbox_consumption(&mut transaction, &consumption, 20_001, 21_000).unwrap(),
        1
    );
    transaction.commit().unwrap();
    consumption
}

fn assert_terminal_lease_evidence_cleared(
    client: &mut Client,
    consumption_ref: &str,
    expected_state: &str,
) {
    let row = client
        .query_one(
            "SELECT consumption_state, claim_expires_at_unix_ms IS NULL, \
                    claim_deadline_at IS NULL \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption_ref],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), expected_state);
    assert!(row.get::<_, bool>(1));
    assert!(row.get::<_, bool>(2));
}

#[test]
fn successful_claimed_terminal_writes_clear_both_lease_deadlines() {
    let (mut client, schema) = isolated_client();

    let completed = claimed_consumption(
        &mut client,
        "event_terminal_complete",
        "consumption_terminal_complete",
    );
    let mut complete = client.transaction().unwrap();
    assert_eq!(
        complete_inbox_consumption(
            &mut complete,
            &completed,
            20_002,
            "completion_projection_applied",
            1,
        )
        .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    complete.commit().unwrap();
    assert_terminal_lease_evidence_cleared(&mut client, completed.consumption_ref(), "completed");

    let quarantined = claimed_consumption(
        &mut client,
        "event_terminal_quarantine",
        "consumption_terminal_quarantine",
    );
    let mut quarantine = client.transaction().unwrap();
    assert_eq!(
        quarantine_inbox_consumption(&mut quarantine, &quarantined, 20_002, "poison_payload", 1,)
            .unwrap(),
        InboxConsumptionDisposition::Inserted
    );
    quarantine.commit().unwrap();
    assert_terminal_lease_evidence_cleared(
        &mut client,
        quarantined.consumption_ref(),
        "quarantined",
    );

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .unwrap();
}
