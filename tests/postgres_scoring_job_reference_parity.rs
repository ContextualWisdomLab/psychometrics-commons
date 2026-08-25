//! `PostgreSQL` scoring-job references must match the Rust opaque-reference boundary.
//!
//! The Rust product boundary trims Unicode outer whitespace and rejects embedded controls plus
//! numeric-like spellings under `char::is_numeric`. Direct SQL and trusted migration upgrades must
//! not retain durable job, request, failure, worker, lease, or result identities that the domain
//! would reject or normalize differently.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;

const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4a4f_4252_4546;

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required")
}

fn guard() -> Client {
    let url = database_url();
    let mut guard = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    guard
        .query_one("SELECT set_config('lock_timeout', $1, false)", &[&"60s"])
        .expect("PostgreSQL lock timeout must be configurable for the scoring-job fixture");
    guard
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("PostgreSQL scoring-job fixture advisory lock should be acquired");
    guard
}

fn client(schema_name: &str) -> Client {
    let url = database_url();
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema_name} CASCADE; \
             CREATE SCHEMA {schema_name}; \
             SET search_path TO {schema_name}, public;"
        ))
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

#[derive(Clone, Copy, Debug)]
enum ReferenceField {
    Job,
    Request,
    Failure,
    Worker,
    Lease,
    Result,
}

fn insert_with_reference(
    client: &mut Client,
    field: ReferenceField,
    reference: &str,
    suffix: usize,
) -> Result<u64, postgres::Error> {
    let job_ref = format!("scoring_job_reference_parity_{suffix}");
    let request_ref = format!("scoring_request_reference_parity_{suffix}");
    let worker_ref = format!("scoring_worker_reference_parity_{suffix}");
    let lease_ref = format!("scoring_lease_reference_parity_{suffix}");

    match field {
        ReferenceField::Job => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) \
             VALUES ($1, $2, 'queued', 0, 3)",
            &[&reference, &request_ref],
        ),
        ReferenceField::Request => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) \
             VALUES ($1, $2, 'queued', 0, 3)",
            &[&job_ref, &reference],
        ),
        ReferenceField::Failure => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, next_attempt_at_unix_ms, last_failure_code) \
             VALUES ($1, $2, 'retry_scheduled', 1, 3, 20000, $3)",
            &[&job_ref, &request_ref, &reference],
        ),
        ReferenceField::Worker => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, active_worker_ref, active_lease_ref, active_fencing_token, active_lease_expires_at_unix_ms) \
             VALUES ($1, $2, 'leased', 1, 3, $3, $4, 1, 20000)",
            &[&job_ref, &request_ref, &reference, &lease_ref],
        ),
        ReferenceField::Lease => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, active_worker_ref, active_lease_ref, active_fencing_token, active_lease_expires_at_unix_ms) \
             VALUES ($1, $2, 'leased', 1, 3, $3, $4, 1, 20000)",
            &[&job_ref, &request_ref, &worker_ref, &reference],
        ),
        ReferenceField::Result => client.execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts, result_ref, completed_fencing_token) \
             VALUES ($1, $2, 'completed', 1, 3, $3, 1)",
            &[&job_ref, &request_ref, &reference],
        ),
    }
}

fn reference_constraint_oids(client: &mut Client) -> Vec<(String, i64)> {
    client
        .query(
            "SELECT conname, oid::bigint \
             FROM pg_constraint \
             WHERE conrelid = 'scoring_job_state'::regclass \
               AND conname = ANY (ARRAY[ \
                   'scoring_job_ref_format_check', \
                   'scoring_request_ref_format_check', \
                   'scoring_failure_code_format_check', \
                   'scoring_worker_ref_format_check', \
                   'scoring_lease_ref_format_check', \
                   'scoring_result_ref_format_check' \
               ]) \
             ORDER BY conname",
            &[],
        )
        .unwrap()
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

#[test]
fn fixture_lock_is_database_visible_and_timeout_bounded() {
    let _guard = guard();
    let url = database_url();
    let mut contender = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    contender
        .query_one("SELECT set_config('lock_timeout', $1, false)", &[&"100ms"])
        .expect("lock timeout must be configurable for the fixture contention probe");
    let error = contender
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect_err("a second PostgreSQL session must not acquire the scoring-job fixture lock");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));
}

#[test]
fn every_scoring_job_reference_rejects_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client("scoring_job_reference_parity_test");
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (field_index, (field, constraint)) in [
        (ReferenceField::Job, "scoring_job_ref_format_check"),
        (ReferenceField::Request, "scoring_request_ref_format_check"),
        (ReferenceField::Failure, "scoring_failure_code_format_check"),
        (ReferenceField::Worker, "scoring_worker_ref_format_check"),
        (ReferenceField::Lease, "scoring_lease_ref_format_check"),
        (ReferenceField::Result, "scoring_result_ref_format_check"),
    ]
    .into_iter()
    .enumerate()
    {
        for (invalid_index, invalid_reference) in invalid_references.into_iter().enumerate() {
            let error = insert_with_reference(
                &mut client,
                field,
                invalid_reference,
                field_index * 10 + invalid_index,
            )
            .expect_err("direct SQL must not bypass the Rust scoring-job reference boundary");
            assert_check(&error, constraint);
        }
    }
}

#[test]
fn reapplying_current_schema_preserves_reference_constraints() {
    let _guard = guard();
    let mut client = client("scoring_job_reference_reapply_test");
    let before = reference_constraint_oids(&mut client);
    assert_eq!(before.len(), 6);

    apply_scoring_job_migration(&mut client)
        .expect("reapplying an already-current scoring-job schema must succeed");

    let after = reference_constraint_oids(&mut client);
    assert_eq!(
        after, before,
        "current reference guards must not be recreated"
    );
}

fn reseal_current_constraint_manifest(client: &mut Client) {
    client
        .batch_execute(
            "DO $seal_manifest$ \
             DECLARE \
                 relation_ref REGCLASS := to_regclass('scoring_job_state'); \
                 actual_constraints TEXT[]; \
                 actual_constraint_manifest TEXT; \
             BEGIN \
                 SELECT ARRAY( \
                     SELECT format( \
                         '%s:%s:%s:%s:%s', \
                         constraint_record.conname, \
                         constraint_record.contype, \
                         constraint_record.convalidated, \
                         constraint_record.conenforced, \
                         pg_get_constraintdef(constraint_record.oid) \
                     ) \
                     FROM pg_constraint AS constraint_record \
                     WHERE constraint_record.conrelid = relation_ref \
                       AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x') \
                     ORDER BY constraint_record.conname \
                 ) INTO actual_constraints; \
                 actual_constraint_manifest := array_to_string(actual_constraints, E'\\n'); \
                 EXECUTE format( \
                     'COMMENT ON TABLE %s IS %L', \
                     relation_ref, \
                     'psychometrics-commons:migration-0002:constraint-manifest:' || actual_constraint_manifest \
                 ); \
             END \
             $seal_manifest$;",
        )
        .unwrap();
}

#[test]
fn trusted_previous_manifest_is_upgraded_to_the_rust_reference_boundary() {
    let _guard = guard();
    let mut client = client("scoring_job_reference_upgrade_test");

    client
        .batch_execute(
            "ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_job_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_job_ref_format_check CHECK ( \
                 scoring_job_ref = btrim(scoring_job_ref) \
                 AND scoring_job_ref <> '' \
                 AND NOT ( \
                     scoring_job_ref ~ '[[:digit:]]' \
                     AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$' \
                 ) \
             );",
        )
        .unwrap();
    reseal_current_constraint_manifest(&mut client);

    apply_scoring_job_migration(&mut client)
        .expect("a trusted previous manifest without invalid rows must upgrade in place");

    let error = insert_with_reference(&mut client, ReferenceField::Job, "½", 900)
        .expect_err("upgraded schemas must reject Rust-invalid Unicode numeric references");
    assert_check(&error, "scoring_job_ref_format_check");
}

#[test]
fn upgrade_fails_closed_when_historical_rows_violate_the_stronger_boundary() {
    let _guard = guard();
    let mut client = client("scoring_job_reference_upgrade_invalid_test");

    client
        .batch_execute(
            "ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_job_ref_format_check; \
             ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_job_ref_format_check CHECK ( \
                 scoring_job_ref = btrim(scoring_job_ref) \
                 AND scoring_job_ref <> '' \
                 AND NOT ( \
                     scoring_job_ref ~ '[[:digit:]]' \
                     AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$' \
                 ) \
             );",
        )
        .unwrap();
    reseal_current_constraint_manifest(&mut client);
    client
        .execute(
            "INSERT INTO scoring_job_state (scoring_job_ref, scoring_request_ref, scoring_state, attempt_count, max_attempts) \
             VALUES ('½', 'scoring_request_historical_invalid', 'queued', 0, 3)",
            &[],
        )
        .expect("the historical predicate should admit the Unicode numeric alias");

    let error = apply_scoring_job_migration(&mut client)
        .expect_err("migration must fail closed while historical invalid identity remains");
    assert_eq!(
        error.as_db_error().map(postgres::error::DbError::code),
        Some(&SqlState::CHECK_VIOLATION)
    );
}
