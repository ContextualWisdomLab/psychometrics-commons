//! Regression contract for immutable inbox-consumption side-effect binding.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, complete_inbox_consumption,
    persist_inbox_consumption, quarantine_inbox_consumption, InboxConsumptionPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS inbox_consumption_binding_test CASCADE;\
             CREATE SCHEMA inbox_consumption_binding_test;\
             SET search_path TO inbox_consumption_binding_test;",
        )
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();
    client
}

fn source_event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_binding",
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

fn consumption(consumption_ref: &str, side_effect_ref: &str) -> InboxConsumption {
    InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_binding",
        consumption_ref,
        side_effect_ref,
        20_000,
    )
    .unwrap()
}

fn persist(client: &mut Client, consumption: &InboxConsumption) {
    let mut transaction = client.transaction().unwrap();
    persist_inbox_consumption(&mut transaction, consumption).unwrap();
    transaction.commit().unwrap();
}

fn state(client: &mut Client, consumption_ref: &str) -> (String, i64) {
    let row = client
        .query_one(
            "SELECT consumption_state, fencing_token FROM integration_consumption \
             WHERE consumer_ref = 'consumer_alpha' \
               AND source_ref = 'psychometrics_commons' \
               AND tenant_ref = 'tenant_alpha' \
               AND source_event_ref = 'event_binding' \
               AND consumption_ref = $1",
            &[&consumption_ref],
        )
        .unwrap();
    (row.get(0), row.get(1))
}

#[test]
fn transitions_reject_rebound_side_effect_identity_without_mutation() {
    let mut client = test_client();
    accept_inbox_event(&mut client, "consumer_alpha", &source_event(), 20_000).unwrap();

    for consumption_ref in ["consumption_claim", "consumption_complete", "consumption_quarantine"] {
        persist(
            &mut client,
            &consumption(consumption_ref, "side_effect_original"),
        );
    }

    let rebound_claim = consumption("consumption_claim", "side_effect_rebound");
    let mut claim_transaction = client.transaction().unwrap();
    assert!(matches!(
        begin_inbox_consumption(&mut claim_transaction, &rebound_claim, 20_001),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    claim_transaction.rollback().unwrap();

    let rebound_complete = consumption("consumption_complete", "side_effect_rebound");
    let mut complete_transaction = client.transaction().unwrap();
    assert!(matches!(
        complete_inbox_consumption(
            &mut complete_transaction,
            &rebound_complete,
            20_001,
            "completion_projection_applied",
            0,
        ),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    complete_transaction.rollback().unwrap();

    let rebound_quarantine = consumption("consumption_quarantine", "side_effect_rebound");
    let mut quarantine_transaction = client.transaction().unwrap();
    assert!(matches!(
        quarantine_inbox_consumption(
            &mut quarantine_transaction,
            &rebound_quarantine,
            20_001,
            "poison_payload",
            0,
        ),
        Err(InboxConsumptionPersistenceError::ConflictingReplay)
    ));
    quarantine_transaction.rollback().unwrap();

    for consumption_ref in ["consumption_claim", "consumption_complete", "consumption_quarantine"] {
        assert_eq!(
            state(&mut client, consumption_ref),
            ("pending".to_owned(), 0),
            "rebound side-effect identity must not mutate durable consumption state"
        );
    }
}
