//! Persist-backed session HTTP uses the sealed stored-release start path.

use std::sync::{Mutex, MutexGuard};

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::apply_assessment_session_migration;
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
};
use psychometrics_commons_runtime::session_http::{
    handle_session_http_request, PostgresSessionHttpPort, SESSION_COLLECTION_PATH,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SCHEMA: &str = "session_http_persistence_test";
static DATABASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_client() -> (MutexGuard<'static, ()>, Client) {
    let guard = DATABASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    (guard, client)
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {SCHEMA}.assessment_session_command;
             DROP TABLE IF EXISTS {SCHEMA}.assessment_session;
             DROP TABLE IF EXISTS {SCHEMA}.instrument_release;"
        ))
        .unwrap();
}

fn manifest(release_ref: &str, digest: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        digest,
    )
    .unwrap()
}

fn approved_evidence(release_ref: &str, digest: &str) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        release_ref,
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        digest,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_050,
            None,
        )
        .unwrap(),
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn published_release(release_ref: &str, digest: &str) -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(release_ref, digest), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_f9f86084",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence(release_ref, digest))
        .unwrap();
    release
        .apply_command(
            "publication_publish_635a7491",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn create_request(session_ref: &str) -> String {
    format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
         Idempotency-Key: {session_ref}\r\n\
         \r\n\
         {{\"participant_ref\":\"{PARTICIPANT_REF}\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}}"
    )
}

#[test]
fn http_create_reloads_after_restart_and_replays_after_suspend() {
    let (_guard, mut client) = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();
    apply_assessment_session_migration(&mut client).unwrap();
    let mut transaction = client.transaction().unwrap();
    persist_instrument_release(
        &mut transaction,
        &published_release("release_big_five_ko_v1", VALID_DIGEST),
    )
    .unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let created = {
        let mut port = PostgresSessionHttpPort::new(&mut transaction);
        handle_session_http_request(&create_request("ses_http_ko_quick"), &mut port, 20_000)
    };
    assert_eq!(created.status(), 201);
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let loaded = {
        let mut port = PostgresSessionHttpPort::new(&mut transaction);
        handle_session_http_request(
            "GET /v1/sessions/ses_http_ko_quick HTTP/1.1\r\n\r\n",
            &mut port,
            20_000,
        )
    };
    assert_eq!(loaded.status(), 200);
    assert_eq!(loaded.body(), created.body());
    transaction.commit().unwrap();

    client
        .execute(
            "UPDATE instrument_release SET publication_state = 'suspended' WHERE release_ref = $1",
            &[&"release_big_five_ko_v1"],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let replay = {
        let mut port = PostgresSessionHttpPort::new(&mut transaction);
        handle_session_http_request(&create_request("ses_http_ko_quick"), &mut port, 20_000)
    };
    assert_eq!(replay.status(), 200);
    let rejected = {
        let mut port = PostgresSessionHttpPort::new(&mut transaction);
        handle_session_http_request(&create_request("ses_http_after_suspend"), &mut port, 21_000)
    };
    assert_eq!(rejected.status(), 409);
    assert!(rejected
        .body()
        .contains("publish the exact instrument release before starting a new session"));
    transaction.commit().unwrap();
}
