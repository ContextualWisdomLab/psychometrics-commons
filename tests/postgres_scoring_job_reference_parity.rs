//! Durable scoring-job identity must match the Rust opaque-reference boundary.
//!
//! Job/request identity, failure classification, active lease ownership, and terminal result
//! evidence are all product-owned references. Direct SQL and a trusted legacy migration must not
//! leave references that `normalized_reference` rejects or normalizes differently.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS scoring_job_reference_parity_test CASCADE; \
             CREATE SCHEMA scoring_job_reference_parity_test; \
             SET search_path TO scoring_job_reference_parity_test, public;",
        )
        .unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_queued(
    client: &mut Client,
    job_ref: &str,
    request_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts\
         ) VALUES ($1,$2,'queued',0,3)",
        &[&job_ref, &request_ref],
    )
}

fn insert_retry(
    client: &mut Client,
    suffix: &str,
    failure_code: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             next_attempt_at_unix_ms, last_failure_code\
         ) VALUES ($1,$2,'retry_scheduled',1,3,20000,$3)",
        &[
            &format!("scoring_job_retry_{suffix}"),
            &format!("scoring_request_retry_{suffix}"),
            &failure_code,
        ],
    )
}

fn insert_leased(
    client: &mut Client,
    suffix: &str,
    worker_ref: &str,
    lease_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             active_worker_ref, active_lease_ref, active_fencing_token,\
             active_lease_expires_at_unix_ms\
         ) VALUES ($1,$2,'leased',1,3,$3,$4,1,20000)",
        &[
            &format!("scoring_job_leased_{suffix}"),
            &format!("scoring_request_leased_{suffix}"),
            &worker_ref,
            &lease_ref,
        ],
    )
}

fn insert_completed(
    client: &mut Client,
    suffix: &str,
    result_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO scoring_job_state (\
             scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts,\
             result_ref, completed_fencing_token\
         ) VALUES ($1,$2,'completed',1,3,$3,1)",
        &[
            &format!("scoring_job_completed_{suffix}"),
            &format!("scoring_request_completed_{suffix}"),
            &result_ref,
        ],
    )
}

#[test]
fn scoring_job_references_reject_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();
    let invalid_references = ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_queued(
            &mut client,
            invalid_ref,
            &format!("scoring_request_job_{index}"),
        )
        .expect_err("job identity must match normalized_reference");
        assert_check(&error, "scoring_job_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_queued(
            &mut client,
            &format!("scoring_job_request_{index}"),
            invalid_ref,
        )
        .expect_err("request identity must match normalized_reference");
        assert_check(&error, "scoring_request_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_retry(&mut client, &index.to_string(), invalid_ref)
            .expect_err("failure classification must match normalized_reference");
        assert_check(&error, "scoring_failure_code_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_leased(
            &mut client,
            &format!("worker_{index}"),
            invalid_ref,
            "scoring_lease_valid",
        )
        .expect_err("worker identity must match normalized_reference");
        assert_check(&error, "scoring_worker_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_leased(
            &mut client,
            &format!("lease_{index}"),
            "worker_valid",
            invalid_ref,
        )
        .expect_err("lease identity must match normalized_reference");
        assert_check(&error, "scoring_lease_ref_format_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_completed(&mut client, &index.to_string(), invalid_ref)
            .expect_err("terminal result identity must match normalized_reference");
        assert_check(&error, "scoring_result_ref_format_check");
    }
}

fn install_trusted_legacy_reference_contract(client: &mut Client) {
    client
        .batch_execute(
            "ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_job_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_job_ref_format_check CHECK (\
                 scoring_job_ref=btrim(scoring_job_ref) AND scoring_job_ref<>'' \
                 AND NOT (scoring_job_ref ~ '[[:digit:]]' AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$')\
             ); \
             ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_request_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_request_ref_format_check CHECK (\
                 scoring_request_ref=btrim(scoring_request_ref) AND scoring_request_ref<>'' \
                 AND NOT (scoring_request_ref ~ '[[:digit:]]' AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$')\
             ); \
             ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_failure_code_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_failure_code_format_check CHECK (\
                 last_failure_code IS NULL OR (last_failure_code=btrim(last_failure_code) \
                 AND last_failure_code<>'' AND NOT (last_failure_code ~ '[[:digit:]]' \
                 AND last_failure_code ~ '^[[:digit:]+,.eE-]+$'))\
             ); \
             ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_worker_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_worker_ref_format_check CHECK (\
                 active_worker_ref IS NULL OR (active_worker_ref=btrim(active_worker_ref) \
                 AND active_worker_ref<>'' AND NOT (active_worker_ref ~ '[[:digit:]]' \
                 AND active_worker_ref ~ '^[[:digit:]+,.eE-]+$'))\
             ); \
             ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_lease_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_lease_ref_format_check CHECK (\
                 active_lease_ref IS NULL OR (active_lease_ref=btrim(active_lease_ref) \
                 AND active_lease_ref<>'' AND NOT (active_lease_ref ~ '[[:digit:]]' \
                 AND active_lease_ref ~ '^[[:digit:]+,.eE-]+$'))\
             ); \
             ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_result_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_result_ref_format_check CHECK (\
                 result_ref IS NULL OR (result_ref=btrim(result_ref) AND result_ref<>'' \
                 AND NOT (result_ref ~ '[[:digit:]]' AND result_ref ~ '^[[:digit:]+,.eE-]+$'))\
             );",
        )
        .unwrap();

    let manifest: String = client
        .query_one(
            "SELECT array_to_string(ARRAY(\
                 SELECT format('%s:%s:%s:%s:%s', conname, contype, convalidated, conenforced,\
                               pg_get_constraintdef(oid))\
                 FROM pg_constraint\
                 WHERE conrelid='scoring_job_state'::regclass\
                   AND contype IN ('c','f','n','p','u','x')\
                 ORDER BY conname\
             ), E'\\n')",
            &[],
        )
        .unwrap()
        .get(0);
    client
        .execute(
            "COMMENT ON TABLE scoring_job_state IS $1",
            &[&format!(
                "psychometrics-commons:migration-0002:constraint-manifest:{manifest}"
            )],
        )
        .unwrap();
}

#[test]
fn trusted_legacy_manifest_is_upgraded_to_the_rust_reference_boundary() {
    let _guard = guard();
    let mut client = client();
    install_trusted_legacy_reference_contract(&mut client);

    insert_queued(&mut client, "½", "scoring_request_legacy_probe")
        .expect("legacy PostgreSQL predicate must admit the regression fixture");
    client
        .execute("DELETE FROM scoring_job_state WHERE scoring_job_ref='½'", &[])
        .unwrap();

    apply_scoring_job_migration(&mut client).unwrap();

    let error = insert_queued(&mut client, "½", "scoring_request_upgrade_guard")
        .expect_err("trusted legacy contract must be upgraded in place");
    assert_check(&error, "scoring_job_ref_format_check");
    apply_scoring_job_migration(&mut client).unwrap();
}

#[test]
fn legacy_upgrade_fails_closed_when_historical_invalid_identity_exists() {
    let _guard = guard();
    let mut client = client();
    install_trusted_legacy_reference_contract(&mut client);
    insert_queued(&mut client, "½", "scoring_request_historical")
        .expect("legacy PostgreSQL predicate must admit the historical regression fixture");

    let error = apply_scoring_job_migration(&mut client)
        .expect_err("migration must reject historical identity that Rust cannot construct");
    assert_eq!(
        error.as_db_error().map(|database_error| database_error.code()),
        Some(&SqlState::CHECK_VIOLATION)
    );
}
