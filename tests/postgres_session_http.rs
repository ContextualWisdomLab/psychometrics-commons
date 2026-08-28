//! Persist-backed session HTTP uses the sealed stored-release start path.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_assessment_session::{
    apply_assessment_session_migration, load_assessment_session_for_participant,
    AssessmentSessionPersistenceError,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
};
use psychometrics_commons_runtime::session_http::{
    handle_authorized_session_http_request, handle_session_http_request, PostgresSessionHttpPort,
    SessionHttpAuthority, SessionHttpPort, SESSION_COLLECTION_PATH,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SCHEMA: &str = "session_http_persistence_test";
const DATABASE_TEST_LOCK_KEY: i64 = 0x5345_5353_4854_5450;

/// Configures a finite wait budget before acquiring a session-scoped `PostgreSQL` advisory lock.
fn acquire_database_lock(
    client: &mut Client,
    lock_key: i64,
    lock_timeout: &str,
) -> Result<(), postgres::Error> {
    client.query_one(
        "SELECT set_config('lock_timeout', $1, false)",
        &[&lock_timeout],
    )?;
    client.query_one("SELECT pg_advisory_lock($1)", &[&lock_key])?;
    Ok(())
}

/// Acquires the cross-process `PostgreSQL` fixture lock and returns the lock-owning
/// session together with a client scoped to the session HTTP test schema.
fn test_client() -> (Client, Client) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    acquire_database_lock(&mut guard, DATABASE_TEST_LOCK_KEY, "60s").expect(
        "PostgreSQL session HTTP fixture advisory lock should be acquired within sixty seconds",
    );
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    (guard, client)
}

/// Clears only the repository-owned tables used by the persisted session HTTP fixture.
fn reset_tables(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {SCHEMA}.assessment_session_command;
             DROP TABLE IF EXISTS {SCHEMA}.assessment_session;
             DROP TABLE IF EXISTS {SCHEMA}.instrument_release;"
        ))
        .unwrap();
}

/// Builds the immutable Korean Big Five release manifest used by this HTTP fixture.
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

/// Builds approved publication evidence for the fixture's exact immutable release.
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

/// Publishes the fixture release through the same evidence-gated domain lifecycle used by runtime code.
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

/// Creates the exact HTTP/1.1 request used to start an anonymous persisted session.
fn create_request(session_ref: &str) -> String {
    format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
         Idempotency-Key: {session_ref}\r\n\
         \r\n\
         {{\"participant_ref\":\"{PARTICIPANT_REF}\",\"instrument_release_ref\":\"release_big_five_ko_v1\",\"locale\":\"ko-KR\"}}"
    )
}

fn authorized_get<P: SessionHttpPort>(
    port: &mut P,
    session_ref: &str,
) -> psychometrics_commons_runtime::session_http::SessionHttpResponse {
    authorized_get_as(port, session_ref, PARTICIPANT_REF, "tenant_session_http")
}

fn authorized_get_as<P: SessionHttpPort>(
    port: &mut P,
    session_ref: &str,
    participant_ref: &str,
    tenant_ref: &str,
) -> psychometrics_commons_runtime::session_http::SessionHttpResponse {
    let participant = ParticipantRecord::new_anonymous(participant_ref, tenant_ref, 1).unwrap();
    let actor = AuthorizationContext::new(
        tenant_ref,
        "subject_session_http",
        Some(participant_ref),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = SessionHttpAuthority::Authenticated(&actor);
    handle_authorized_session_http_request(
        &format!("GET /v1/sessions/{session_ref} HTTP/1.1\r\n\r\n"),
        &authority,
        &participant,
        port,
        20_000,
    )
}

/// Proves that fixture serialization is enforced by `PostgreSQL` across separate client sessions.
#[test]
fn fixture_lock_is_visible_across_database_sessions() {
    let (_guard, _owner) = test_client();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&DATABASE_TEST_LOCK_KEY],
        )
        .unwrap()
        .get(0);

    if acquired {
        contender
            .query_one("SELECT pg_advisory_unlock($1)", &[&DATABASE_TEST_LOCK_KEY])
            .unwrap();
    }

    assert!(
        !acquired,
        "session HTTP fixture serialization must be visible to separate PostgreSQL sessions"
    );
}

/// Proves fixture acquisition cannot wait forever behind a stalled lock owner.
#[test]
fn fixture_lock_wait_has_finite_postgresql_budget() {
    let (mut guard, _owner) = test_client();
    let timeout_ms: i64 = guard
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name = 'lock_timeout'",
            &[],
        )
        .expect("session HTTP fixture lock timeout should be queryable from PostgreSQL")
        .get(0);

    assert_eq!(
        timeout_ms, 60_000,
        "session HTTP fixture must not wait indefinitely for its PostgreSQL advisory lock"
    );
}

/// Proves `PostgreSQL` itself aborts a contended advisory-lock wait at the configured budget.
#[test]
fn fixture_lock_wait_aborts_under_real_contention() {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut holder = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let behavior_lock_key: i64 = holder
        .query_one("SELECT pg_backend_pid()::bigint", &[])
        .expect("holder backend identity should be queryable")
        .get(0);
    holder
        .query_one("SELECT pg_advisory_lock($1)", &[&behavior_lock_key])
        .expect("behavior-test holder should acquire its private advisory lock");

    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let error = acquire_database_lock(&mut contender, behavior_lock_key, "100ms")
        .expect_err("contended session HTTP fixture lock must stop at the configured timeout");
    assert_eq!(error.code(), Some(&SqlState::LOCK_NOT_AVAILABLE));

    let released: bool = holder
        .query_one("SELECT pg_advisory_unlock($1)", &[&behavior_lock_key])
        .expect("behavior-test advisory lock should be released")
        .get(0);
    assert!(released, "behavior-test advisory lock should be released");
}

/// Proves persisted session replay survives restart while new starts fail after release suspension.
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
    let (loaded, foreign) = {
        let mut port = PostgresSessionHttpPort::new(&mut transaction);
        let loaded = authorized_get(&mut port, "ses_http_ko_quick");
        let foreign = authorized_get_as(
            &mut port,
            "ses_http_ko_quick",
            "ptc_foreign_session_http",
            "tenant_session_http",
        );
        (loaded, foreign)
    };
    assert_eq!(loaded.status(), 200);
    assert_eq!(loaded.body(), created.body());
    assert_eq!(foreign.status(), 404);
    assert!(!foreign.body().contains(PARTICIPANT_REF));
    assert!(matches!(
        load_assessment_session_for_participant(
            &mut transaction,
            "ses_http_ko_quick",
            " ptc_eb1b318917d24ca0ac5153c37ff696c7",
        ),
        Err(AssessmentSessionPersistenceError::InvalidReference)
    ));
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
