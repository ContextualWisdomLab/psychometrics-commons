//! Real `PostgreSQL` contract for participant data-rights backlog health evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::BacklogHealth;
use psychometrics_commons_runtime::postgres_data_rights::apply_data_rights_migration;
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_data_rights_backlog, probe_postgres_data_rights_backlog,
    DataRightsBacklogPolicy, PostgresBacklogProbeError,
};
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
    let schema = format!("data_rights_backlog_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated data-rights backlog schema should be created");
    apply_integration_migration(&mut client).expect("integration migration should apply");
    apply_data_rights_migration(&mut client).expect("data-rights migration should apply");
    (client, schema)
}

fn cleanup(mut client: Client, schema: &str) {
    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated data-rights backlog schema should be removed");
}

fn policy() -> DataRightsBacklogPolicy {
    DataRightsBacklogPolicy {
        max_active_request_count: 4,
        max_active_request_age_ms: 10_000,
        max_pending_propagation_count: 4,
        max_pending_propagation_age_ms: 5_000,
        max_quarantined_propagation_count: 1,
    }
}

fn insert_request(
    client: &mut Client,
    suffix: &str,
    state: &str,
    requested_at: i64,
    latest_event_at: i64,
) {
    let request_ref = format!("request_{suffix}");
    client
        .execute(
            "INSERT INTO data_rights_request_state (\
                 request_ref, tenant_ref, participant_ref, request_kind, scope_ref,\
                 current_state, requested_at_unix_ms, latest_event_at_unix_ms\
             ) VALUES ($1,'tenant_rights_alpha','participant_rights_alpha','export',\
                       'scope_rights_alpha',$2,$3,$4)",
            &[&request_ref, &state, &requested_at, &latest_event_at],
        )
        .unwrap();
}

fn insert_propagation(client: &mut Client, suffix: &str, state: &str, event_at: i64) {
    let request_ref = format!("request_propagation_{suffix}");
    let event_ref = format!("event_propagation_{suffix}");
    let dependent_system_ref = format!("dependent_system_{suffix}");
    insert_request(client, &format!("propagation_{suffix}"), "processing", 1_000, 1_500);
    client
        .execute(
            "INSERT INTO integration_outbox (\
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,\
                 occurred_at_unix_ms, correlation_ref, payload_digest, max_attempts,\
                 current_state, latest_event_at_unix_ms\
             ) VALUES ($1,'participant.data_rights.changed','v1','psychometrics_commons',\
                       'tenant_rights_alpha','participant_rights_alpha',$2,\
                       'correlation_rights_alpha',\
                       'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',\
                       3,'pending',$2)",
            &[&event_ref, &event_at],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO data_rights_propagation_state (\
                 request_ref, tenant_ref, dependent_system_ref, source_ref, event_ref,\
                 current_state, latest_event_at_unix_ms\
             ) VALUES ($1,'tenant_rights_alpha',$2,'psychometrics_commons',$3,$4,$5)",
            &[
                &request_ref,
                &dependent_system_ref,
                &event_ref,
                &state,
                &event_at,
            ],
        )
        .unwrap();
}

#[test]
fn empty_data_rights_backlog_is_within_caller_supplied_bounds() {
    let (mut client, schema) = isolated_client();
    let evidence = probe_postgres_data_rights_backlog(&mut client).unwrap();

    assert_eq!(evidence.active_request_count(), 0);
    assert_eq!(evidence.pending_propagation_count(), 0);
    assert_eq!(evidence.quarantined_propagation_count(), 0);
    assert_eq!(evidence.oldest_active_request_at_unix_ms(), None);
    assert_eq!(evidence.oldest_pending_propagation_event_at_unix_ms(), None);
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 20_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn probe_keeps_request_age_separate_from_propagation_age() {
    let (mut client, schema) = isolated_client();
    insert_request(&mut client, "requested_alpha", "requested", 2_000, 4_000);
    insert_request(
        &mut client,
        "identity_verified_alpha",
        "identity_verified",
        3_000,
        5_000,
    );
    insert_request(&mut client, "completed_alpha", "completed", 500, 6_000);
    insert_propagation(&mut client, "pending_alpha", "pending", 7_000);
    insert_propagation(
        &mut client,
        "quarantined_alpha",
        "quarantined",
        8_000,
    );

    let evidence = probe_postgres_data_rights_backlog(&mut client).unwrap();
    assert_eq!(evidence.active_request_count(), 4);
    assert_eq!(evidence.pending_propagation_count(), 1);
    assert_eq!(evidence.quarantined_propagation_count(), 1);
    assert_eq!(evidence.oldest_active_request_at_unix_ms(), Some(1_000));
    assert_eq!(
        evidence.oldest_pending_propagation_event_at_unix_ms(),
        Some(7_000)
    );
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 8_500, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn participant_rights_backlog_fails_closed_by_count_age_or_quarantine_policy() {
    let (mut client, schema) = isolated_client();
    insert_request(&mut client, "old_active_alpha", "processing", 2_000, 3_000);
    insert_propagation(&mut client, "old_pending_alpha", "pending", 4_000);
    insert_propagation(
        &mut client,
        "quarantined_policy_alpha",
        "quarantined",
        4_500,
    );
    let evidence = probe_postgres_data_rights_backlog(&mut client).unwrap();

    let strict_request_count = DataRightsBacklogPolicy {
        max_active_request_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 5_000, &strict_request_count),
        BacklogHealth::Stalled
    );

    let strict_propagation_count = DataRightsBacklogPolicy {
        max_pending_propagation_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 5_000, &strict_propagation_count),
        BacklogHealth::Stalled
    );

    let strict_quarantine = DataRightsBacklogPolicy {
        max_quarantined_propagation_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 5_000, &strict_quarantine),
        BacklogHealth::Stalled
    );

    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 12_001, &policy()),
        BacklogHealth::Stalled
    );

    let request_age_relaxed = DataRightsBacklogPolicy {
        max_active_request_age_ms: 20_000,
        ..policy()
    };
    assert_eq!(
        classify_postgres_data_rights_backlog(&evidence, 9_001, &request_age_relaxed),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn future_data_rights_evidence_is_unknown_instead_of_healthy() {
    let (mut client, schema) = isolated_client();
    insert_request(&mut client, "future_request_alpha", "requested", 10_000, 10_000);
    let request_evidence = probe_postgres_data_rights_backlog(&mut client).unwrap();
    assert_eq!(
        classify_postgres_data_rights_backlog(&request_evidence, 9_999, &policy()),
        BacklogHealth::Unknown
    );
    assert_eq!(
        classify_postgres_data_rights_backlog(&request_evidence, 0, &policy()),
        BacklogHealth::Unknown
    );

    cleanup(client, &schema);

    let (mut client, schema) = isolated_client();
    insert_propagation(&mut client, "future_propagation_alpha", "pending", 12_000);
    let propagation_evidence = probe_postgres_data_rights_backlog(&mut client).unwrap();
    assert_eq!(
        classify_postgres_data_rights_backlog(&propagation_evidence, 11_999, &policy()),
        BacklogHealth::Unknown
    );
    cleanup(client, &schema);
}

#[test]
fn data_rights_probe_rejects_invalid_stored_time_and_surfaces_database_failure() {
    let mut client = test_client();
    let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
    let schema = format!("data_rights_backlog_invalid_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};\
             CREATE TABLE data_rights_request_state (\
                 current_state TEXT, requested_at_unix_ms BIGINT);\
             CREATE TABLE data_rights_propagation_state (\
                 current_state TEXT, latest_event_at_unix_ms BIGINT);\
             INSERT INTO data_rights_request_state VALUES ('requested', -1);"
        ))
        .unwrap();
    let error = probe_postgres_data_rights_backlog(&mut client).unwrap_err();
    assert!(matches!(error, PostgresBacklogProbeError::InvalidStoredValue));
    assert!(error.source().is_none());

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    transaction.batch_execute("SELECT 1 / 0").unwrap_err();
    let error = probe_postgres_data_rights_backlog(&mut transaction).unwrap_err();
    assert!(matches!(error, PostgresBacklogProbeError::Database(_)));
    assert!(error.source().is_some());
}
