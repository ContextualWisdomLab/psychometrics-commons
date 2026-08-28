//! Real PostgreSQL contract for database-authoritative inbox claim expiry.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::{InboxConsumption, IntegrationEvent};
use psychometrics_commons_runtime::postgres_inbox_consumption::{
    apply_inbox_consumption_migration, begin_inbox_consumption, expire_inbox_consumption,
    persist_inbox_consumption, InboxConsumptionPersistenceError,
};
use psychometrics_commons_runtime::postgres_integration::{
    accept_inbox_event, apply_integration_migration,
};
use std::ops::{Deref, DerefMut};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct SchemaClient {
    client: Client,
    schema_name: String,
}

impl Deref for SchemaClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for SchemaClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Drop for SchemaClient {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "RESET search_path; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema_name
        ));
    }
}

fn isolated_client() -> SchemaClient {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let database_nonce: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for test isolation")
        .get(0);
    let schema_name = format!("inbox_database_deadline_{database_nonce}");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .expect("isolated inbox deadline schema should be created");
    let mut client = SchemaClient {
        client,
        schema_name,
    };
    apply_integration_migration(&mut *client).expect("integration migration should apply");
    apply_inbox_consumption_migration(&mut *client)
        .expect("inbox-consumption migrations should apply atomically");
    client
}

fn source_event() -> IntegrationEvent {
    IntegrationEvent::new(
        "event_database_deadline",
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

fn pending() -> InboxConsumption {
    InboxConsumption::pending(
        "consumer_alpha",
        "psychometrics_commons",
        "tenant_alpha",
        "event_database_deadline",
        "consumption_database_deadline",
        "side_effect_projection",
        20_000,
    )
    .unwrap()
}

#[test]
fn future_caller_timestamp_cannot_expire_a_live_database_claim() {
    let mut client = isolated_client();
    accept_inbox_event(&mut *client, "consumer_alpha", &source_event(), 20_000).unwrap();
    let consumption = pending();

    let mut claim = client.transaction().unwrap();
    persist_inbox_consumption(&mut claim, &consumption).unwrap();
    assert_eq!(
        begin_inbox_consumption(&mut claim, &consumption, 20_001, 80_001).unwrap(),
        1
    );
    claim.commit().unwrap();

    let database_deadline_is_future: bool = client
        .query_one(
            "SELECT claim_deadline_at > clock_timestamp() \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption.consumption_ref()],
        )
        .unwrap()
        .get(0);
    assert!(
        database_deadline_is_future,
        "the fixture must hold a live database-authoritative lease"
    );

    let mut expire = client.transaction().unwrap();
    assert!(matches!(
        expire_inbox_consumption(&mut expire, &consumption, 80_001),
        Err(InboxConsumptionPersistenceError::ConsumptionClaimStillActive)
    ));
    expire.rollback().unwrap();

    let row = client
        .query_one(
            "SELECT consumption_state, fencing_token, claim_expires_at_unix_ms, \
                    claim_deadline_at > clock_timestamp() \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption.consumption_ref()],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "processing");
    assert_eq!(row.get::<_, i64>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(80_001));
    assert!(row.get::<_, bool>(3));
}
