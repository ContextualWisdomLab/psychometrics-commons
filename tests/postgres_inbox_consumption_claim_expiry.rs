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
use std::ops::{Deref, DerefMut};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEGACY_INBOX_CONSUMPTION_MIGRATION: &str =
    include_str!("../migrations/0012_integration_consumption.sql");

struct SchemaClient {
    client: Client,
    schema_name: String,
}

impl SchemaClient {
    fn schema_name(&self) -> &str {
        &self.schema_name
    }
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

fn schema_client(prefix: &str) -> SchemaClient {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let database_nonce: String = client
        .query_one("SELECT pg_current_xact_id()::text", &[])
        .expect("PostgreSQL must allocate a durable transaction identity for test isolation")
        .get(0);
    let schema_name = format!("{prefix}_{database_nonce}");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
        ))
        .unwrap();
    let mut client = SchemaClient {
        client,
        schema_name,
    };
    apply_integration_migration(&mut client).unwrap();
    client
}

fn isolated_client() -> SchemaClient {
    let mut client = schema_client("inbox_claim_expiry");
    apply_inbox_consumption_migration(&mut client).unwrap();
    client
}

fn legacy_isolated_client() -> SchemaClient {
    let mut client = schema_client("inbox_claim_upgrade");
    client
        .batch_execute(LEGACY_INBOX_CONSUMPTION_MIGRATION)
        .unwrap();
    client
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

fn database_now_unix_ms(client: &mut Client) -> u64 {
    let now: i64 = client
        .query_one(
            "SELECT floor(EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT",
            &[],
        )
        .unwrap()
        .get(0);
    u64::try_from(now).expect("database clock must be after the Unix epoch")
}

fn prepare_claim_with_window(
    client: &mut Client,
    event_ref: &str,
    consumption_ref: &str,
    observed_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> InboxConsumption {
    accept_inbox_event(client, "consumer_alpha", &source_event(event_ref), 20_000).unwrap();
    let consumption = pending(event_ref, consumption_ref);
    let mut transaction = client.transaction().unwrap();
    persist_inbox_consumption(&mut transaction, &consumption).unwrap();
    let fence = begin_inbox_consumption(
        &mut transaction,
        &consumption,
        observed_at_unix_ms,
        expires_at_unix_ms,
    )
    .unwrap();
    assert_eq!(fence, 1);
    transaction.commit().unwrap();
    consumption
}

fn prepare_claim(client: &mut Client, event_ref: &str, consumption_ref: &str) -> InboxConsumption {
    prepare_claim_with_window(client, event_ref, consumption_ref, 20_001, 21_000)
}

fn assert_processing_with_expiry(client: &mut Client, consumption_ref: &str, expected_expiry: i64) {
    let row = client
        .query_one(
            "SELECT consumption_state, fencing_token, claim_expires_at_unix_ms, \
                    claim_deadline_at IS NOT NULL \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption_ref],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "processing");
    assert_eq!(row.get::<_, i64>(1), 1);
    assert_eq!(row.get::<_, Option<i64>>(2), Some(expected_expiry));
    assert!(row.get::<_, bool>(3));
}

fn assert_processing(client: &mut Client, consumption_ref: &str) {
    assert_processing_with_expiry(client, consumption_ref, 21_000);
}

#[test]
fn database_transaction_identity_prevents_schema_name_reuse() {
    let first = schema_client("inbox_claim_schema_identity");
    let second = schema_client("inbox_claim_schema_identity");

    assert_ne!(
        first.schema_name(),
        second.schema_name(),
        "schema isolation must not depend on a process-local counter or PID lifetime"
    );
}

#[test]
fn expired_claim_cannot_complete_at_or_after_expiry_with_current_fence() {
    let mut client = isolated_client();
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
}

#[test]
fn expired_claim_cannot_quarantine_at_or_after_expiry_with_current_fence() {
    let mut client = isolated_client();
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
}

#[test]
fn database_clock_rejects_expired_claim_with_stale_pre_expiry_caller_time() {
    let mut client = isolated_client();
    let database_now = database_now_unix_ms(&mut client);
    let claimed_at = database_now
        .checked_sub(2_000)
        .expect("database clock must be at least two seconds after the Unix epoch");
    let expired_at = claimed_at + 1;
    let stale_observed_at = claimed_at;

    let completion = prepare_claim_with_window(
        &mut client,
        "event_expired_stale_complete",
        "consumption_expired_stale_complete",
        claimed_at,
        expired_at,
    );
    client.simple_query("SELECT pg_sleep(0.02)").unwrap();
    let mut complete = client.transaction().unwrap();
    assert!(
        complete_inbox_consumption(
            &mut complete,
            &completion,
            stale_observed_at,
            "completion_with_stale_clock",
            1,
        )
        .is_err(),
        "database time, not a stale caller timestamp, must fence an expired completion"
    );
    complete.rollback().unwrap();
    assert_processing_with_expiry(
        &mut client,
        completion.consumption_ref(),
        i64::try_from(expired_at).unwrap(),
    );

    let quarantine = prepare_claim_with_window(
        &mut client,
        "event_expired_stale_quarantine",
        "consumption_expired_stale_quarantine",
        claimed_at,
        expired_at,
    );
    client.simple_query("SELECT pg_sleep(0.02)").unwrap();
    let mut quarantine_transaction = client.transaction().unwrap();
    assert!(
        quarantine_inbox_consumption(
            &mut quarantine_transaction,
            &quarantine,
            stale_observed_at,
            "poison_payload",
            1,
        )
        .is_err(),
        "database time, not a stale caller timestamp, must fence an expired quarantine"
    );
    quarantine_transaction.rollback().unwrap();
    assert_processing_with_expiry(
        &mut client,
        quarantine.consumption_ref(),
        i64::try_from(expired_at).unwrap(),
    );
}

#[test]
fn forward_migration_fails_closed_for_preexisting_processing_claim() {
    let mut client = legacy_isolated_client();
    let consumption = prepare_claim(
        &mut client,
        "event_preexisting_processing",
        "consumption_preexisting_processing",
    );

    let legacy_column_count: i64 = client
        .query_one(
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_schema = current_schema() \
               AND table_name = 'integration_consumption' \
               AND column_name = 'claim_deadline_at'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(legacy_column_count, 0);

    apply_inbox_consumption_migration(&mut client).unwrap();
    apply_inbox_consumption_migration(&mut client).unwrap();

    let upgraded = client
        .query_one(
            "SELECT consumption_state, fencing_token, claim_expires_at_unix_ms, \
                    claim_deadline_at IS NOT NULL \
             FROM integration_consumption WHERE consumption_ref = $1",
            &[&consumption.consumption_ref()],
        )
        .unwrap();
    assert_eq!(upgraded.get::<_, String>(0), "processing");
    assert_eq!(upgraded.get::<_, i64>(1), 1);
    assert_eq!(upgraded.get::<_, Option<i64>>(2), Some(21_000));
    assert!(upgraded.get::<_, bool>(3));

    let mut transaction = client.transaction().unwrap();
    assert!(
        complete_inbox_consumption(
            &mut transaction,
            &consumption,
            20_002,
            "completion_after_upgrade",
            1,
        )
        .is_err(),
        "an in-flight claim without trustworthy wall-clock provenance must fail closed after upgrade"
    );
    transaction.rollback().unwrap();
    assert_processing(&mut client, consumption.consumption_ref());
}

#[test]
fn forward_migrations_roll_back_together_when_hardening_cannot_apply() {
    let mut client = schema_client("inbox_claim_atomic_upgrade");
    client
        .batch_execute(
            "CREATE FUNCTION maintain_inbox_claim_deadline() RETURNS text \
             LANGUAGE sql IMMUTABLE AS $$ SELECT 'conflict'::text $$;",
        )
        .unwrap();

    let error = apply_inbox_consumption_migration(&mut client)
        .expect_err("a conflicting guard function signature must reject the migration batch");
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::INVALID_FUNCTION_DEFINITION),
        "the fixture must fail at the guard function replacement boundary"
    );

    let base_table_exists: bool = client
        .query_one(
            "SELECT to_regclass('integration_consumption') IS NOT NULL",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        !base_table_exists,
        "the shipped base migration must roll back when forward hardening fails"
    );
}
