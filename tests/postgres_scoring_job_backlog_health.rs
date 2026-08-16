//! Real `PostgreSQL` contract for scoring-job backlog health evidence.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::health::BacklogHealth;
use psychometrics_commons_runtime::postgres_health::{
    classify_postgres_scoring_job_backlog, probe_postgres_scoring_job_backlog,
    PostgresBacklogProbeError, ScoringJobBacklogPolicy,
};
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;
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
    let schema = format!("scoring_job_backlog_{}_{}", std::process::id(), nonce);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .expect("isolated scoring-job backlog schema should be created");
    apply_scoring_job_migration(&mut client).expect("scoring-job migration should apply");
    (client, schema)
}

fn cleanup(mut client: Client, schema: &str) {
    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .expect("isolated scoring-job backlog schema should be removed");
}

fn policy() -> ScoringJobBacklogPolicy {
    ScoringJobBacklogPolicy {
        max_active_job_count: 4,
        max_active_job_age_ms: 5_000,
        max_quarantined_job_count: 1,
        max_expired_lease_count: 1,
    }
}

fn insert_job(client: &mut Client, job_ref: &str, state: &str, created_at_unix_ms: i64) {
    let request_ref = format!("scoring_request_{job_ref}");
    let created_at = format!(
        "TIMESTAMPTZ '1970-01-01 00:00:00+00' + INTERVAL '{created_at_unix_ms} milliseconds'"
    );
    match state {
        "queued" => {
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, created_at, updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'queued', 0, 3, {created_at}, {created_at}\
                     )"
                ))
                .unwrap();
        }
        "leased" | "leased_live" => {
            let expiry = if state == "leased_live" {
                4_102_444_800_000i64
            } else {
                created_at_unix_ms + 1_000
            };
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, active_worker_ref, active_lease_ref, active_fencing_token,\
                         active_lease_expires_at_unix_ms, created_at, updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'leased', 1, 3, 'worker_{job_ref}',\
                         'lease_{job_ref}', 1, {expiry}, {created_at}, {created_at}\
                     )"
                ))
                .unwrap();
        }
        "retry_scheduled" => {
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, next_attempt_at_unix_ms, last_failure_code, created_at,\
                         updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'retry_scheduled', 1, 3, {next_attempt},\
                         'retryable_failure', {created_at}, {created_at}\
                     )",
                    next_attempt = created_at_unix_ms + 500
                ))
                .unwrap();
        }
        "quarantined" => {
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, last_failure_code, created_at, updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'quarantined', 1, 3, 'poison_failure',\
                         {created_at}, {created_at}\
                     )"
                ))
                .unwrap();
        }
        "completed" => {
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, result_ref, completed_fencing_token, created_at, updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'completed', 1, 3, 'result_{job_ref}', 1,\
                         {created_at}, {created_at}\
                     )"
                ))
                .unwrap();
        }
        "cancelled" => {
            client
                .batch_execute(&format!(
                    "INSERT INTO scoring_job_state (\
                         scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                         max_attempts, created_at, updated_at\
                     ) VALUES (\
                         '{job_ref}', '{request_ref}', 'cancelled', 0, 3, {created_at}, {created_at}\
                     )"
                ))
                .unwrap();
        }
        other => panic!("unsupported scoring-job test state: {other}"),
    }
}

fn index_definition(client: &mut Client, index_name: &str) -> String {
    client
        .query_one(
            "SELECT indexdef FROM pg_indexes \
             WHERE schemaname = current_schema() AND indexname = $1",
            &[&index_name],
        )
        .unwrap_or_else(|error| panic!("index {index_name} must exist: {error}"))
        .get(0)
}

#[test]
fn empty_scoring_job_backlog_is_observable_without_inventing_service_levels() {
    let (mut client, schema) = isolated_client();
    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();

    assert_eq!(evidence.active_job_count(), 0);
    assert_eq!(evidence.quarantined_job_count(), 0);
    assert_eq!(evidence.expired_lease_count(), 0);
    assert_eq!(evidence.oldest_active_job_at_unix_ms(), None);
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 10_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn probe_counts_queued_leased_retry_and_quarantine_without_terminal_or_identity_data() {
    let (mut client, schema) = isolated_client();
    insert_job(&mut client, "scoring_job_queued_alpha", "queued", 2_000);
    insert_job(&mut client, "scoring_job_leased_alpha", "leased", 2_500);
    insert_job(
        &mut client,
        "scoring_job_retry_alpha",
        "retry_scheduled",
        3_000,
    );
    insert_job(
        &mut client,
        "scoring_job_quarantined_alpha",
        "quarantined",
        3_500,
    );
    insert_job(
        &mut client,
        "scoring_job_completed_alpha",
        "completed",
        1_000,
    );
    insert_job(
        &mut client,
        "scoring_job_cancelled_alpha",
        "cancelled",
        1_500,
    );

    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();
    assert_eq!(evidence.active_job_count(), 3);
    assert_eq!(evidence.quarantined_job_count(), 1);
    assert_eq!(evidence.expired_lease_count(), 1);
    assert_eq!(evidence.oldest_active_job_at_unix_ms(), Some(2_000));
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 5_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn caller_policy_can_fail_scoring_backlog_closed_by_count_or_age() {
    let (mut client, schema) = isolated_client();
    insert_job(&mut client, "scoring_job_policy_queued", "queued", 2_000);
    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();

    let strict_count = ScoringJobBacklogPolicy {
        max_active_job_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 4_000, &strict_count),
        BacklogHealth::Stalled
    );

    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 7_001, &policy()),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn quarantine_limit_is_an_independent_operator_policy_input() {
    let (mut client, schema) = isolated_client();
    insert_job(
        &mut client,
        "scoring_job_policy_quarantine",
        "quarantined",
        2_000,
    );
    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();

    let strict_quarantine = ScoringJobBacklogPolicy {
        max_quarantined_job_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 3_000, &strict_quarantine),
        BacklogHealth::Stalled
    );

    cleanup(client, &schema);
}

#[test]
fn expired_lease_count_fails_closed_when_operator_refuses_dead_workers() {
    let (mut client, schema) = isolated_client();
    insert_job(
        &mut client,
        "scoring_job_expired_lease_alpha",
        "leased",
        8_000,
    );
    insert_job(
        &mut client,
        "scoring_job_live_lease_alpha",
        "leased_live",
        8_500,
    );
    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();

    assert_eq!(evidence.active_job_count(), 2);
    assert_eq!(evidence.expired_lease_count(), 1);
    assert_eq!(evidence.oldest_active_job_at_unix_ms(), Some(8_000));

    let refuse_expired = ScoringJobBacklogPolicy {
        max_expired_lease_count: 0,
        ..policy()
    };
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 10_000, &refuse_expired),
        BacklogHealth::Stalled
    );
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 10_000, &policy()),
        BacklogHealth::WithinBounds
    );

    cleanup(client, &schema);
}

#[test]
fn future_or_missing_observation_time_is_unknown_not_falsely_healthy() {
    let (mut client, schema) = isolated_client();
    insert_job(&mut client, "scoring_job_future_alpha", "queued", 10_000);
    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();

    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 0, &policy()),
        BacklogHealth::Unknown
    );
    assert_eq!(
        classify_postgres_scoring_job_backlog(&evidence, 9_999, &policy()),
        BacklogHealth::Unknown
    );

    cleanup(client, &schema);
}

#[test]
fn non_positive_created_at_fails_closed_instead_of_normalizing() {
    let (mut client, schema) = isolated_client();
    client
        .batch_execute(
            "INSERT INTO scoring_job_state (\
                 scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                 max_attempts, created_at, updated_at\
             ) VALUES (\
                 'scoring_job_invalid_created', 'scoring_request_invalid_created', 'queued', 0, 3,\
                 TIMESTAMPTZ '1970-01-01 00:00:00+00', TIMESTAMPTZ '1970-01-01 00:00:00+00'\
             )",
        )
        .unwrap();

    assert!(matches!(
        probe_postgres_scoring_job_backlog(&mut client),
        Err(PostgresBacklogProbeError::InvalidStoredValue)
    ));

    cleanup(client, &schema);
}

#[test]
fn sub_millisecond_created_at_rounds_instead_of_truncating_to_invalid() {
    let (mut client, schema) = isolated_client();
    client
        .batch_execute(
            "INSERT INTO scoring_job_state (\
                 scoring_job_ref, scoring_request_ref, scoring_state, attempt_count,\
                 max_attempts, created_at, updated_at\
             ) VALUES (\
                 'scoring_job_fractional_created', 'scoring_request_fractional_created', 'queued',\
                 0, 3,\
                 TIMESTAMPTZ '1970-01-01 00:00:00+00' + INTERVAL '0.6 milliseconds',\
                 TIMESTAMPTZ '1970-01-01 00:00:00+00' + INTERVAL '0.6 milliseconds'\
             )",
        )
        .unwrap();

    let evidence = probe_postgres_scoring_job_backlog(&mut client).unwrap();
    assert_eq!(evidence.active_job_count(), 1);
    assert_eq!(evidence.oldest_active_job_at_unix_ms(), Some(1));

    cleanup(client, &schema);
}

#[test]
fn scoring_job_apply_path_creates_partial_readiness_indexes_idempotently() {
    let (mut client, schema) = isolated_client();

    for index_name in [
        "scoring_job_state_active_health_idx",
        "scoring_job_state_quarantined_health_idx",
        "scoring_job_state_leased_expiry_health_idx",
    ] {
        let definition = index_definition(&mut client, index_name).to_ascii_lowercase();
        assert!(
            definition.contains("scoring_job_state"),
            "{index_name} must index scoring_job_state: {definition}"
        );
        assert!(
            definition.contains(" where "),
            "{index_name} must be partial so terminal history does not dominate readiness: {definition}"
        );
        assert!(
            definition.contains("scoring_state"),
            "{index_name} predicate must constrain scoring_state: {definition}"
        );
    }
    for index_name in [
        "scoring_job_state_active_health_idx",
        "scoring_job_state_quarantined_health_idx",
    ] {
        let definition = index_definition(&mut client, index_name).to_ascii_lowercase();
        assert!(
            definition.contains("created_at"),
            "{index_name} must cover created_at: {definition}"
        );
    }

    let active =
        index_definition(&mut client, "scoring_job_state_active_health_idx").to_ascii_lowercase();
    for state in ["queued", "leased", "retry_scheduled"] {
        assert!(
            active.contains(state),
            "active readiness index must include {state}: {active}"
        );
    }
    let quarantined = index_definition(&mut client, "scoring_job_state_quarantined_health_idx")
        .to_ascii_lowercase();
    assert!(
        quarantined.contains("quarantined"),
        "quarantine readiness index must include quarantined: {quarantined}"
    );
    let leased_expiry = index_definition(&mut client, "scoring_job_state_leased_expiry_health_idx")
        .to_ascii_lowercase();
    assert!(
        leased_expiry.contains("active_lease_expires_at_unix_ms"),
        "expired-lease readiness index must cover lease expiry: {leased_expiry}"
    );
    assert!(
        leased_expiry.contains("leased"),
        "expired-lease readiness index must constrain leased rows: {leased_expiry}"
    );

    apply_scoring_job_migration(&mut client)
        .expect("scoring-job health index apply must be idempotent");

    cleanup(client, &schema);
}

#[test]
fn scoring_job_probe_supports_transaction_success_and_direct_client_failure() {
    let (mut client, schema) = isolated_client();

    let mut transaction = client.transaction().unwrap();
    let evidence = probe_postgres_scoring_job_backlog(&mut transaction).unwrap();
    assert_eq!(evidence.active_job_count(), 0);
    transaction.rollback().unwrap();

    client
        .batch_execute("DROP TABLE scoring_job_state CASCADE;")
        .unwrap();
    let error = probe_postgres_scoring_job_backlog(&mut client).unwrap_err();
    assert!(matches!(error, PostgresBacklogProbeError::Database(_)));
    assert_eq!(error.to_string(), "PostgreSQL backlog probe failed");
    assert!(error.source().is_some());

    cleanup(client, &schema);
}
