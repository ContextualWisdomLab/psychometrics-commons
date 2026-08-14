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
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            "SELECT participant_ref, instrument_release_ref, instrument_release_content_digest,
                    locale, session_state, created_at_unix_ms
             FROM assessment_session WHERE session_ref = $1",
            &[&"ses_02fe09e373504b7986ae78491116edbd"],
        )
        .unwrap();
    let participant: String = row.get(0);
    let release: String = row.get(1);
    let digest: String = row.get(2);
    let locale: String = row.get(3);
    let state: String = row.get(4);
    let created_at: i64 = row.get(5);
    assert_eq!(participant, PARTICIPANT_REF);
    assert_eq!(release, "release_big_five_ko_v1");
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
    assert!(matches!(
        persist_assessment_session(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::Database(_))
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
    client
        .batch_execute("ALTER TABLE assessment_session DROP COLUMN locale;")
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
