//! Scoring dispatch outbox evidence must be causally bound to the request and job being persisted.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;
use psychometrics_commons_runtime::postgres_scoring_job::apply_scoring_job_migration;
use psychometrics_commons_runtime::postgres_scoring_request::{
    apply_scoring_request_migration, persist_scoring_dispatch, ScoringDispatchPersistenceError,
};
#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::scoring_job::ScoringJob;
use response_support::frozen_snapshot;

const SCHEMA: &str = "scoring_dispatch_envelope_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5343_4453_5045_4E56;
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn ready_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared scoring-dispatch envelope test lock should be acquired");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
             CREATE SCHEMA {SCHEMA};
             SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    apply_integration_migration(&mut client).unwrap();
    apply_scoring_job_migration(&mut client).unwrap();
    apply_scoring_request_migration(&mut client).unwrap();
    client
}

fn request() -> ScoringRequest {
    let snapshot = frozen_snapshot(
        "session_dispatch_envelope_alpha",
        "response_snapshot_dispatch_envelope_alpha",
        &[ResponseWrite {
            server_event_ref: "server_event_dispatch_envelope_alpha",
            client_event_ref: "client_event_dispatch_envelope_alpha",
            item_version_ref: "item_version_dispatch_envelope_alpha",
            payload_digest: DIGEST,
        }],
    );
    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_dispatch_envelope_alpha",
            response_snapshot_ref: "response_snapshot_dispatch_envelope_alpha",
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

fn event(source: &str, subject: &str, causation: Option<&str>) -> IntegrationEvent {
    IntegrationEvent::new(
        "event_scoring_dispatch_envelope_alpha",
        "scoring.dispatch.requested",
        "v1",
        source,
        "tenant_dispatch_envelope_alpha",
        subject,
        10_000,
        "correlation_dispatch_envelope_alpha",
        causation,
        DIGEST,
    )
    .unwrap()
}

#[test]
fn unrelated_source_subject_or_snapshot_causation_is_rejected_before_writes() {
    let cases = [
        (
            "other_source",
            "scoring_job_dispatch_envelope_alpha",
            Some("response_snapshot_dispatch_envelope_alpha"),
        ),
        (
            "psychometrics_commons",
            "scoring_job_dispatch_envelope_other",
            Some("response_snapshot_dispatch_envelope_alpha"),
        ),
        (
            "psychometrics_commons",
            "scoring_job_dispatch_envelope_alpha",
            Some("response_snapshot_dispatch_envelope_other"),
        ),
        (
            "psychometrics_commons",
            "scoring_job_dispatch_envelope_alpha",
            None,
        ),
    ];

    for (index, (source, subject, causation)) in cases.into_iter().enumerate() {
        let mut client = ready_client();
        let request = request();
        let job = ScoringJob::new(
            "scoring_job_dispatch_envelope_alpha",
            request.scoring_request_ref(),
            3,
        )
        .unwrap();
        let dispatch_event = event(source, subject, causation);

        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist_scoring_dispatch(&mut transaction, &request, &job, &dispatch_event, 3),
            Err(ScoringDispatchPersistenceError::InvalidDispatchEnvelope)
        ));
        transaction.rollback().unwrap();

        for table in ["scoring_request", "scoring_job_state", "integration_outbox"] {
            let count: i64 = client
                .query_one(&format!("SELECT count(*) FROM {table}"), &[])
                .unwrap()
                .get(0);
            assert_eq!(
                count, 0,
                "invalid envelope case {index} must not write {table}"
            );
        }
    }

    let mut client = ready_client();
    client
        .batch_execute(&format!(
            "SET search_path TO public;
             DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"
        ))
        .unwrap();
}
