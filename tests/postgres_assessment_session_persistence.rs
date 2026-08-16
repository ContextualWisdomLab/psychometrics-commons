//! Real `PostgreSQL` contract for created assessment-session identity.

use std::sync::{Mutex, MutexGuard};

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::{
    apply_assessment_session_migration, persist_assessment_session,
    AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError,
};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SCHEMA: &str = "assessment_session_persistence_test";
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

fn reset_session_table(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {SCHEMA}.assessment_session;"
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

fn created_session(
    session_ref: &str,
    participant_ref: &str,
    release_ref: &str,
    digest: &str,
) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        participant_ref,
        &published_release(release_ref, digest),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

#[test]
fn created_session_persists_release_binding_and_replays_exactly() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_02fe09e373504b7986ae78491116edbd",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        persist_assessment_session(&mut transaction, &session).unwrap(),
        AssessmentSessionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_assessment_session(&mut transaction, &session).unwrap(),
        AssessmentSessionPersistenceDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let row = client
        .query_one(
            "SELECT participant_ref, instrument_release_ref, instrument_version_ref,
                    instrument_release_content_digest, locale, session_state,
                    created_at_unix_ms
             FROM assessment_session WHERE session_ref = $1",
            &[&"ses_02fe09e373504b7986ae78491116edbd"],
        )
        .unwrap();
    let participant: String = row.get(0);
    let release: String = row.get(1);
    let version: String = row.get(2);
    let digest: String = row.get(3);
    let locale: String = row.get(4);
    let state: String = row.get(5);
    let created_at: i64 = row.get(6);
    assert_eq!(participant, PARTICIPANT_REF);
    assert_eq!(release, "release_big_five_ko_v1");
    assert_eq!(version, "instrument_version_big_five_ko_v1");
    assert_eq!(digest, VALID_DIGEST);
    assert_eq!(locale, "ko-KR");
    assert_eq!(state, "created");
    assert_eq!(created_at, 20_000);
}

#[test]
fn conflicting_session_identity_and_non_created_state_fail_closed() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_conflict_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let other_participant = created_session(
        "ses_conflict_alpha",
        "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let other_release = created_session(
        "ses_conflict_alpha",
        PARTICIPANT_REF,
        "release_big_five_en_v1",
        OTHER_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &other_participant),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    assert!(matches!(
        persist_assessment_session(&mut transaction, &other_release),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.commit().unwrap();

    for (column, value) in [
        ("instrument_version_ref", "instrument_version_conflict_v1"),
        ("instrument_release_content_digest", OTHER_DIGEST),
        ("locale", "en-US"),
        ("session_state", "active"),
    ] {
        client
            .execute(
                &format!("UPDATE assessment_session SET {column} = $2 WHERE session_ref = $1"),
                &[&"ses_conflict_alpha", &value],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(
            matches!(
                persist_assessment_session(&mut transaction, &session),
                Err(AssessmentSessionPersistenceError::ConflictingReplay)
            ),
            "replay must fail closed when stored {column} is rebound"
        );
        transaction.rollback().unwrap();
        client
            .execute(
                "UPDATE assessment_session
                 SET instrument_version_ref = $2,
                     instrument_release_content_digest = $3,
                     locale = $4,
                     session_state = $5
                 WHERE session_ref = $1",
                &[
                    &"ses_conflict_alpha",
                    &"instrument_version_big_five_ko_v1",
                    &VALID_DIGEST,
                    &"ko-KR",
                    &"created",
                ],
            )
            .unwrap();
    }

    client
        .execute(
            "UPDATE assessment_session SET created_at_unix_ms = $2 WHERE session_ref = $1",
            &[&"ses_conflict_alpha", &30_000_i64],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut activated = session;
    activated
        .apply_command("cmd_activate_session", 1, SessionCommand::Activate)
        .unwrap();
    assert_eq!(activated.state(), SessionState::Active);
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &activated),
        Err(AssessmentSessionPersistenceError::UnsupportedInitialState)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn session_persist_requires_read_committed_and_surfaces_database_failure() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_isolation_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();

    reset_session_table(&mut client);
    let mut transaction = client.transaction().unwrap();
    let missing_table = persist_assessment_session(&mut transaction, &session).unwrap_err();
    assert_eq!(
        missing_table.to_string(),
        "PostgreSQL assessment-session persistence failed"
    );
    assert!(std::error::Error::source(&missing_table).is_some());
    transaction.rollback().unwrap();

    let overflow = AssessmentSession::new(
        "ses_overflow_alpha",
        PARTICIPANT_REF,
        &published_release("release_big_five_ko_v1", VALID_DIGEST),
        "ko-KR",
        u64::MAX,
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &overflow),
        Err(AssessmentSessionPersistenceError::ValueOutOfRange)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn classify_select_failure_after_conflict_is_a_database_failure() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_classify_hidden",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_assessment_session(&mut transaction, &session).unwrap();
        transaction.commit().unwrap();
    }
    let sink = format!("assessment_session_classify_sink_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {sink} CASCADE;
             CREATE SCHEMA {sink};
             CREATE OR REPLACE FUNCTION {SCHEMA}.assessment_session_redirect_after_insert()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER assessment_session_redirect_after_insert
             AFTER INSERT ON {SCHEMA}.assessment_session
             FOR EACH STATEMENT EXECUTE FUNCTION {SCHEMA}.assessment_session_redirect_after_insert();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let result = persist_assessment_session(&mut transaction, &session);
    transaction.rollback().unwrap();

    client
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA};
             DROP TRIGGER IF EXISTS assessment_session_redirect_after_insert ON {SCHEMA}.assessment_session;
             DROP FUNCTION IF EXISTS {SCHEMA}.assessment_session_redirect_after_insert();
             DROP SCHEMA IF EXISTS {sink} CASCADE;"
        ))
        .unwrap();

    let error = result.expect_err("replay classify-select must return the database error");
    assert!(matches!(
        error,
        AssessmentSessionPersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL assessment-session persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn isolation_query_failure_is_a_database_failure() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_isolation_query_hidden",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let mut transaction = client.transaction().unwrap();
    assert!(transaction
        .batch_execute("SELECT * FROM assessment_session_isolation_query_missing")
        .is_err());
    let error = persist_assessment_session(&mut transaction, &session)
        .expect_err("aborted isolation probe must return the database error");
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AssessmentSessionPersistenceError::Database(_)
    ));
    assert_eq!(
        error.to_string(),
        "PostgreSQL assessment-session persistence failed"
    );
    assert!(std::error::Error::source(&error).is_some());
}
