//! Real `PostgreSQL` contract for durable scoring-request identity.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_request, ScoringRequestPersistenceDisposition,
    ScoringRequestPersistenceError,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::session::SessionState;
use response_support::{advance_to, active_session};

/// Freeze one session-bound completed snapshot through the authoritative ledger API.
fn frozen_snapshot(
    session_ref: &str,
    snapshot_ref: &str,
    writes: &[ResponseWrite<'_>],
) -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut session = active_session(session_ref);
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    for request in writes {
        ledger.record(&session, *request).unwrap();
    }
    advance_to(&mut session, SessionState::Completed);
    ledger.freeze_as(&session, snapshot_ref).unwrap()
}

const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4F52_5251_5053;

fn scoring_request_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL scoring request lock should be acquired");
    client
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_request_persistence_test;\
             SET search_path TO scoring_request_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_scoring_request_tables(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS scoring_request_persistence_test.scoring_request;")
        .unwrap();
}

fn persist_ok(
    client: &mut Client,
    request: &ScoringRequest,
) -> ScoringRequestPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_scoring_request(&mut transaction, request).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(client: &mut Client, request: &ScoringRequest) -> ScoringRequestPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_scoring_request(&mut transaction, request).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn request_named(
    session_ref: &str,
    scoring_request_ref: &str,
    snapshot_ref: &str,
    scoring_version_ref: &str,
    norm_version_ref: Option<&str>,
) -> ScoringRequest {
    let snapshot = frozen_snapshot(
        session_ref,
        snapshot_ref,
        &[ResponseWrite {
            server_event_ref: "server_event_score_one",
            client_event_ref: "client_event_score_one",
            item_version_ref: "item_version_001",
            payload_digest: PAYLOAD_DIGEST,
        }],
    );
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref,
            response_snapshot_ref: snapshot_ref,
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref,
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

#[test]
fn fixed_schema_serialization_must_be_visible_to_other_database_sessions() {
    let _guard = scoring_request_test_guard();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .expect("cross-process fixture lock should be observable from PostgreSQL")
        .get(0);
    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .expect("RED fixture lock should be released after probing");
    }
    assert!(
        !acquired,
        "a process-local mutex cannot serialize a fixed PostgreSQL schema across CI processes"
    );
}

#[test]
fn scoring_request_persist_is_exactly_idempotent_and_version_rebinding_fails_closed() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_score_alpha",
        "scoring_request_alpha",
        "response_snapshot_alpha",
        "scoring_version_big_five_v1",
        Some("norm_version_big_five_ko_v1"),
    );
    assert_eq!(
        persist_ok(&mut client, &request),
        ScoringRequestPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &request),
        ScoringRequestPersistenceDisposition::Duplicate
    );

    let rebound = request_named(
        "session_score_alpha",
        "scoring_request_alpha",
        "response_snapshot_alpha",
        "scoring_version_big_five_v2",
        Some("norm_version_big_five_ko_v1"),
    );
    assert!(matches!(
        persist_err(&mut client, &rebound),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
}

#[test]
fn stored_field_mismatches_fail_closed() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_score_fields",
        "scoring_request_fields",
        "response_snapshot_fields",
        "scoring_version_big_five_v1",
        Some("norm_version_big_five_ko_v1"),
    );
    persist_ok(&mut client, &request);

    client
        .execute(
            "UPDATE scoring_request SET session_ref = 'session_score_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET session_ref = 'session_score_fields', \
                 response_snapshot_ref = 'response_snapshot_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET response_snapshot_ref = 'response_snapshot_fields', \
                 assessment_spec_ref = 'assessment_spec_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET assessment_spec_ref = 'assessment_spec_big_five_v1', \
                 instrument_version_ref = 'instrument_version_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET instrument_version_ref = 'instrument_version_big_five_ko_v1', \
                 calibration_reference = 'calibration_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET calibration_reference = 'calibration_big_five_ko_v1', \
                 norm_version_ref = 'norm_version_other' \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE scoring_request SET norm_version_ref = 'norm_version_big_five_ko_v1', \
                 requested_output_schema_version = 9 \
             WHERE scoring_request_ref = 'scoring_request_fields'",
            &[],
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
}

#[test]
fn absent_norm_persists_and_rebinding_a_norm_fails_closed() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_score_no_norm",
        "scoring_request_no_norm",
        "response_snapshot_no_norm",
        "scoring_version_big_five_v1",
        None,
    );
    assert_eq!(
        persist_ok(&mut client, &request),
        ScoringRequestPersistenceDisposition::Inserted
    );
    let with_norm = request_named(
        "session_score_no_norm",
        "scoring_request_no_norm",
        "response_snapshot_no_norm",
        "scoring_version_big_five_v1",
        Some("norm_version_big_five_ko_v1"),
    );
    assert!(matches!(
        persist_err(&mut client, &with_norm),
        ScoringRequestPersistenceError::ConflictingReplay
    ));
}

#[test]
fn scoring_request_persistence_requires_read_committed() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_score_serializable",
        "scoring_request_serializable",
        "response_snapshot_serializable",
        "scoring_version_big_five_v1",
        None,
    );
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_scoring_request(&mut transaction, &request),
        Err(ScoringRequestPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_scoring_request_relation_is_a_database_failure() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);

    let request = request_named(
        "session_score_missing",
        "scoring_request_missing",
        "response_snapshot_missing",
        "scoring_version_big_five_v1",
        None,
    );
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::Database(_)
    ));
}

#[test]
fn replay_select_failure_is_a_database_failure() {
    let _guard = scoring_request_test_guard();
    let mut client = test_client();
    reset_scoring_request_tables(&mut client);
    apply_scoring_request_migration(&mut client).unwrap();

    let request = request_named(
        "session_score_hidden",
        "scoring_request_hidden",
        "response_snapshot_hidden",
        "scoring_version_big_five_v1",
        None,
    );
    persist_ok(&mut client, &request);
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS scoring_request_select_failure_sink;\
             CREATE OR REPLACE FUNCTION scoring_request_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'scoring_request_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER scoring_request_redirect_after_insert \
             AFTER INSERT ON scoring_request \
             FOR EACH STATEMENT EXECUTE FUNCTION scoring_request_redirect_after_insert();",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &request),
        ScoringRequestPersistenceError::Database(_)
    ));
}
