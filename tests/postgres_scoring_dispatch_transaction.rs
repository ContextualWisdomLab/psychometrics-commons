//! Real `PostgreSQL` contract for atomic scoring-dispatch persistence.

use postgres::error::SqlState;
use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::{
    apply_integration_migration, enqueue_outbox_event, PersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_job::{
    apply_scoring_job_migration, ScoringJobPersistenceDisposition,
};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_dispatch, ScoringDispatchPersistenceError,
    ScoringRequestPersistenceDisposition,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use response_support::frozen_snapshot;

const PAYLOAD_DIGEST_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PAYLOAD_DIGEST_B: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4453_5054_584e;

fn database_connection() -> String {
    std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database")
}

fn acquire_dispatch_test_guard(lock_timeout: &str) -> Client {
    let connection = database_connection();
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .query_one(
            "SELECT set_config('lock_timeout', $1, false)",
            &[&lock_timeout],
        )
        .expect("PostgreSQL lock timeout must be configurable for the scoring dispatch fixture");
    guard
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("PostgreSQL scoring dispatch fixture advisory lock should be acquired");
    guard
}

fn dispatch_test_guard() -> Client {
    acquire_dispatch_test_guard("60s")
}

fn test_client() -> Client {
    let connection = database_connection();
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_dispatch_transaction_test;\
             SET search_path TO scoring_dispatch_transaction_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS integration_delivery_attempt;\
             DROP TABLE IF EXISTS integration_inbox;\
             DROP TABLE IF EXISTS integration_outbox;\
             DROP TABLE IF EXISTS scoring_job_state;\
             DROP TABLE IF EXISTS scoring_request;",
        )
        .unwrap();
}

fn apply_migrations(client: &mut Client) {
    apply_integration_migration(client).unwrap();
    apply_scoring_job_migration(client).unwrap();
    apply_scoring_request_migration(client).unwrap();
}

fn request_named(
    session_ref: &str,
    scoring_request_ref: &str,
    snapshot_ref: &str,
) -> ScoringRequest {
    let snapshot = frozen_snapshot(
        session_ref,
        snapshot_ref,
        &[ResponseWrite {
            server_event_ref: "server_event_dispatch_one",
            client_event_ref: "client_event_dispatch_one",
            item_version_ref: "item_version_dispatch_one",
            payload_digest: PAYLOAD_DIGEST_A,
        }],
    );
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn dispatch_event(
    event_ref: &str,
    digest: &str,
    scoring_job_ref: &str,
    response_snapshot_ref: &str,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "scoring.dispatch.requested",
        "v1",
        "psychometrics_commons",
        "tenant_dispatch_alpha",
        scoring_job_ref,
        10_000,
        "correlation_dispatch_alpha",
        Some(response_snapshot_ref),
        digest,
    )
    .unwrap()
}

#[test]
fn fixture_lock_is_database_visible_and_timeout_bounded() {
    let _guard = dispatch_test_guard();
    let connection = database_connection();
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    contender
        .query_one("SELECT set_config('lock_timeout', $1, false)", &[&"100ms"])
        .expect("lock timeout must be configurable for the fixture contention probe");
    let error = contender
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect_err("a second PostgreSQL session must not acquire the fixture lock");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));
}

#[test]
fn request_job_and_outbox_are_committed_and_replayed_as_one_dispatch() {
    let _guard = dispatch_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_migrations(&mut client);

    let request = request_named(
        "session_dispatch_alpha",
        "scoring_request_dispatch_alpha",
        "response_snapshot_dispatch_alpha",
    );
    let job = ScoringJob::new(
        "scoring_job_dispatch_alpha",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let event = dispatch_event(
        "event_scoring_dispatch_alpha",
        PAYLOAD_DIGEST_A,
        job.scoring_job_ref(),
        request.response_snapshot_ref(),
    );

    let mut transaction = client.transaction().unwrap();
    let inserted = persist_scoring_dispatch(&mut transaction, &request, &job, &event, 3).unwrap();
    assert_eq!(
        inserted.scoring_request(),
        ScoringRequestPersistenceDisposition::Inserted
    );
    assert_eq!(
        inserted.scoring_job(),
        ScoringJobPersistenceDisposition::Inserted
    );
    assert_eq!(inserted.outbox(), PersistenceDisposition::Inserted);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let duplicate = persist_scoring_dispatch(&mut transaction, &request, &job, &event, 3).unwrap();
    assert_eq!(
        duplicate.scoring_request(),
        ScoringRequestPersistenceDisposition::Duplicate
    );
    assert_eq!(
        duplicate.scoring_job(),
        ScoringJobPersistenceDisposition::Duplicate
    );
    assert_eq!(duplicate.outbox(), PersistenceDisposition::Duplicate);
    transaction.commit().unwrap();

    for table in ["scoring_request", "scoring_job_state", "integration_outbox"] {
        let count: i64 = client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .unwrap()
            .get(0);
        assert_eq!(count, 1, "{table} must contain exactly one durable row");
    }
}

#[test]
fn mismatched_job_request_fails_before_any_write() {
    let _guard = dispatch_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_migrations(&mut client);

    let request = request_named(
        "session_dispatch_mismatch",
        "scoring_request_dispatch_mismatch",
        "response_snapshot_dispatch_mismatch",
    );
    let job = ScoringJob::new("scoring_job_dispatch_alpha", "other_scoring_request", 3).unwrap();
    let event = dispatch_event(
        "event_scoring_dispatch_mismatch",
        PAYLOAD_DIGEST_A,
        job.scoring_job_ref(),
        request.response_snapshot_ref(),
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_dispatch(&mut transaction, &request, &job, &event, 3),
        Err(ScoringDispatchPersistenceError::MismatchedScoringRequest)
    ));
    transaction.rollback().unwrap();

    for table in ["scoring_request", "scoring_job_state", "integration_outbox"] {
        let count: i64 = client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .unwrap()
            .get(0);
        assert_eq!(
            count, 0,
            "{table} must remain empty after rejected dispatch"
        );
    }
}

#[test]
fn outbox_conflict_rolls_back_request_and_job_insertions() {
    let _guard = dispatch_test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_migrations(&mut client);

    let request = request_named(
        "session_dispatch_conflict",
        "scoring_request_dispatch_conflict",
        "response_snapshot_dispatch_conflict",
    );
    let job = ScoringJob::new(
        "scoring_job_dispatch_alpha",
        request.scoring_request_ref(),
        3,
    )
    .unwrap();
    let existing_event = dispatch_event(
        "event_scoring_dispatch_conflict",
        PAYLOAD_DIGEST_A,
        job.scoring_job_ref(),
        request.response_snapshot_ref(),
    );
    assert_eq!(
        enqueue_outbox_event(&mut client, &existing_event, 3).unwrap(),
        PersistenceDisposition::Inserted
    );
    let conflicting_event = dispatch_event(
        "event_scoring_dispatch_conflict",
        PAYLOAD_DIGEST_B,
        job.scoring_job_ref(),
        request.response_snapshot_ref(),
    );

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_scoring_dispatch(&mut transaction, &request, &job, &conflicting_event, 3),
        Err(ScoringDispatchPersistenceError::Outbox(_))
    ));
    transaction.rollback().unwrap();

    let request_count: i64 = client
        .query_one(
            "SELECT count(*) FROM scoring_request WHERE scoring_request_ref = $1",
            &[&request.scoring_request_ref()],
        )
        .unwrap()
        .get(0);
    let job_count: i64 = client
        .query_one(
            "SELECT count(*) FROM scoring_job_state WHERE scoring_job_ref = $1",
            &[&job.scoring_job_ref()],
        )
        .unwrap()
        .get(0);
    let outbox_count: i64 = client
        .query_one(
            "SELECT count(*) FROM integration_outbox WHERE event_ref = $1",
            &[&existing_event.event_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(request_count, 0);
    assert_eq!(job_count, 0);
    assert_eq!(
        outbox_count, 1,
        "pre-existing outbox evidence must remain intact"
    );
}
