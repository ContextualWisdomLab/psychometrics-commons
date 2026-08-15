//! Real `PostgreSQL` contract for integration-backlog health evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::BacklogHealth;
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_integration_backlog, probe_postgres_integration_backlog,
    IntegrationBacklogPolicy, PostgresBacklogProbeError,
};
use psychometrics_commons_runtime::postgres_inbox_consumption::apply_inbox_consumption_migration;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};

static SCHEMA_NONCE: AtomicU64 = AtomicU64::new(1);

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn isolated_client() -> (Client, String) {
    let mut client = test_client();
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!(
        "integration_backlog_health_{}_{}",
        std::process::id(),
        nonce
    );
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated backlog-health schema should be created");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_inbox_consumption_migration(&mut client)
        .expect("inbox consumption migration should apply");
    (client, schema)
}

fn cleanup(mut client: Client, schema: &str) {
    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated backlog-health schema should be removed");
}

fn policy() -> IntegrationBacklogPolicy {
    IntegrationBacklogPolicy {
        max_pending_outbox_count: 4,
        max_pending_outbox_age_ms: 5_000,
        max_quarantined_outbox_count: 1,
        max_active_consumption_count: 4,
        max_active_consumption_age_ms: 5_000,
        max_quarantined_consumption_count: 1,
    }
}

fn insert_outbox(client: &mut Client, event_ref: &str, state: &str, event_at: i64) {
    client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
                 occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
                 current_state, latest_event_at_unix_ms\
             ) VALUES ($1,'assessment.result.changed','v1','psychometrics_commons',\
                       'tenant_backlog_alpha','subject_backlog_alpha',$2,\
                       'correlation_backlog_alpha',\
                       'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',\
                       3,$3,$2)",
            &[&event_ref, &event_at, &state],
        )
        .unwrap();
}

fn insert_consumption(client: &mut Client, suffix: &str, state: &str, event_at: i64) {
    let source_event_ref = format!("source_event_{suffix}");
    let consumption_ref = format!("consumption_{suffix}");
    let side_effect_ref = format!("side_effect_{suffix}");
    client
        .execute(
            "INSERT INTO integration_inbox (\
                 consumer_ref, source_ref, tenant_ref, source_event_ref, event_type,\
                 schema_version, subject_ref, payload_digest, received_at_unix_ms\
             ) VALUES ('consumer_backlog_alpha','upstream_backlog_alpha',\
                       'tenant_backlog_alpha',$1,'assessment.result.changed','v1',\
                       'subject_backlog_alpha',\
                       'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',$2)",
            &[&source_event_ref, &event_at],
        )
        .unwrap();

    match state {
        "pending" => {
            client
                .execute(
                    "INSERT INTO integration_consumption (\
                         consumer_ref, source_ref, tenant_ref, source_event_ref,\
                         consumption_ref, side_effect_ref, consumption_state, fencing_token,\
                         latest_event_at_unix_ms\
                     ) VALUES ('consumer_backlog_alpha','upstream_backlog_alpha',\
                               'tenant_backlog_alpha',$1,$2,$3,'pending',0,$4)",
                    &[&source_event_ref, &consumption_ref, &side_effect_ref, &event_at],
                )
                .unwrap();
        }
        "processing" => {
            let claim_expires_at = event_at + 1_000;
            client
                .execute(
                    "INSERT INTO integration_consumption (\
                         consumer_ref, source_ref, tenant_ref, source_event_ref,\
                         consumption_ref, side_effect_ref, consumption_state, fencing_token,\
                         latest_event_at_unix_ms, claim_expires_at_unix_ms\
                     ) VALUES ('consumer_backlog_alpha','upstream_backlog_alpha',\
                               'tenant_backlog_alpha',$1,$2,$3,'processing',1,$4,$5)",
                    &[
                        &source_event_ref,
                        &consumption_ref,
                        &side_effect_ref,
                        &event_at,
                        &claim_expires_at,
                    ],
                )
                .unwrap();
        }
        "quarantined" => {
            let cause_code = format!("cause_{suffix}");
            client
                .execute(
                    "INSERT INTO integration_consumption (\
                         consumer_ref, source_ref, tenant_ref, source_event_ref,\
                         consumption_ref, side_effect_ref, consumption_state, fencing_token,\
                         latest_event_at_unix_ms, cause_code\
                     ) VALUES ('consumer_backlog_alpha','upstream_backlog_alpha',\
                               'tenant_backlog_alpha',$1,$2,$3,'quarantined',0,$4,$5)",
                    &[
                        &source_event_ref,
                        &consumption_ref,
                        &side_effect_ref,
                        &event_at,
                        &cause_code,
                    ],
                )
                .unwrap();
        }
        other => panic!("unsupported test state: {other}"),
    }
}

#[test]
fn empty_integration_backlog_is_observable_without_inventing_service_levels() {
    let (mut client, schema) = isolated_client();
    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();

    assert_eq!(evidence.pending_outbox_count(), 0);
    assert_eq!(evidence.quarantined_outbox_count(), 0);
    assert_eq!(evidence.active_consumption_count(), 0);
    assert_eq!(evidence.quarantined_consumption_count(), 0);
    assert_eq!(evidence.oldest_pending_outbox_event_at_unix_ms(), None);
    assert_eq!(evidence.oldest_active_consumption_event_at_unix_ms(), None);
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 10_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn probe_reports_pending_processing_and_quarantined_work_without_payload_data() {
    let (mut client, schema) = isolated_client();
    insert_outbox(&mut client, "event_pending_alpha", "pending", 2_000);
    insert_outbox(&mut client, "event_quarantined_alpha", "quarantined", 2_500);
    insert_consumption(&mut client, "pending_alpha", "pending", 3_000);
    insert_consumption(&mut client, "processing_alpha", "processing", 3_500);
    insert_consumption(&mut client, "quarantined_alpha", "quarantined", 4_000);

    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();
    assert_eq!(evidence.pending_outbox_count(), 1);
    assert_eq!(evidence.quarantined_outbox_count(), 1);
    assert_eq!(evidence.active_consumption_count(), 2);
    assert_eq!(evidence.quarantined_consumption_count(), 1);
    assert_eq!(
        evidence.oldest_pending_outbox_event_at_unix_ms(),
        Some(2_000)
    );
    assert_eq!(
        evidence.oldest_active_consumption_event_at_unix_ms(),
        Some(3_000)
    );
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 5_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn caller_policy_can_fail_backlog_closed_by_count_or_age_without_hard_coded_slo() {
    let (mut client, schema) = isolated_client();
    insert_outbox(&mut client, "event_pending_policy", "pending", 2_000);
    insert_consumption(&mut client, "pending_policy", "pending", 3_000);
    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();

    let strict_outbox_count = IntegrationBacklogPolicy {
        max_pending_outbox_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 4_000, &strict_outbox_count),
        BacklogHealth::Stalled
    );

    let strict_consumption_count = IntegrationBacklogPolicy {
        max_active_consumption_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 4_000, &strict_consumption_count),
        BacklogHealth::Stalled
    );

    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 8_001, &policy()),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn quarantine_limits_are_independent_operator_policy_inputs() {
    let (mut client, schema) = isolated_client();
    insert_outbox(
        &mut client,
        "event_quarantined_policy",
        "quarantined",
        2_000,
    );
    insert_consumption(
        &mut client,
        "quarantined_policy",
        "quarantined",
        2_500,
    );
    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();

    let strict_outbox_quarantine = IntegrationBacklogPolicy {
        max_quarantined_outbox_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 3_000, &strict_outbox_quarantine),
        BacklogHealth::Stalled
    );

    let strict_consumption_quarantine = IntegrationBacklogPolicy {
        max_quarantined_consumption_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 3_000, &strict_consumption_quarantine),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn future_or_missing_observation_time_is_unknown_not_falsely_healthy() {
    let (mut client, schema) = isolated_client();
    insert_outbox(&mut client, "event_future_alpha", "pending", 10_000);
    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();

    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 0, &policy()),
        BacklogHealth::Unknown
    );
    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 9_999, &policy()),
        BacklogHealth::Unknown
    );

    cleanup(client, &schema);
}

#[test]
fn active_consumption_age_can_independently_stall_readiness() {
    let (mut client, schema) = isolated_client();
    insert_consumption(&mut client, "old_processing_alpha", "processing", 2_000);
    let evidence = probe_postgres_integration_backlog(&mut client).unwrap();

    assert_eq!(
        classify_postgres_integration_backlog(&evidence, 7_001, &policy()),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn probe_rejects_invalid_stored_timestamps_and_keeps_database_errors_typed() {
    let mut client = test_client();
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!("integration_backlog_invalid_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};\
             CREATE TABLE integration_outbox (current_state TEXT, latest_event_at_unix_ms BIGINT);\
             CREATE TABLE integration_consumption (consumption_state TEXT, latest_event_at_unix_ms BIGINT);\
             INSERT INTO integration_outbox VALUES ('pending', -1);"
        ))
        .unwrap();

    let error = probe_postgres_integration_backlog(&mut client).unwrap_err();
    assert!(matches!(error, PostgresBacklogProbeError::InvalidStoredValue));
    assert!(error.source().is_none());

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    transaction.batch_execute("SELECT 1 / 0").unwrap_err();
    let error = probe_postgres_integration_backlog(&mut transaction).unwrap_err();
    assert!(matches!(error, PostgresBacklogProbeError::Database(_)));
    assert!(error.source().is_some());
}
