//! Real `PostgreSQL` contract for created assessment-session identity.

use std::fmt::Write;
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::{
    apply_assessment_session_migration, load_assessment_session, persist_assessment_session,
    persist_assessment_session_commands, start_created_assessment_session,
    AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError,
    AssessmentSessionStartError,
};
use psychometrics_commons_runtime::session::{
    AssessmentSession, SessionCommand, SessionCreationError, SessionState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const SCHEMA: &str = "assessment_session_persistence_test";
static DATABASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    DATABASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA}; SET search_path TO {SCHEMA};"
        ))
        .unwrap();
    client
}

fn test_client() -> (MutexGuard<'static, ()>, Client) {
    (test_guard(), connect_client())
}

fn reset_session_table(client: &mut Client) {
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {SCHEMA}.assessment_session_command;
             DROP TABLE IF EXISTS {SCHEMA}.assessment_session;"
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

    let mut load_transaction = client.transaction().unwrap();
    let loaded = load_assessment_session(
        &mut load_transaction,
        "ses_02fe09e373504b7986ae78491116edbd",
    )
    .unwrap()
    .expect("persisted created session must be loadable");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.session_ref(), session.session_ref());
    assert_eq!(loaded.participant_ref(), session.participant_ref());
    assert_eq!(
        loaded.instrument_release_ref(),
        session.instrument_release_ref()
    );
    assert_eq!(
        loaded.instrument_version_ref(),
        session.instrument_version_ref()
    );
    assert_eq!(
        loaded.instrument_release_content_digest(),
        session.instrument_release_content_digest()
    );
    assert_eq!(loaded.locale(), session.locale());
    assert_eq!(loaded.created_at_unix_ms(), session.created_at_unix_ms());
    assert_eq!(loaded.state(), SessionState::Created);
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
fn created_session_load_restores_identity_and_rejects_later_states() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let session = created_session(
        "ses_load_created_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(
        load_assessment_session(&mut transaction, "ses_missing_created_session")
            .unwrap()
            .is_none()
    );
    transaction.commit().unwrap();

    for later_state in [
        "active",
        "paused",
        "completed",
        "scoring",
        "scored",
        "released",
        "expired",
        "cancelled",
        "invalidated",
    ] {
        client
            .execute(
                "UPDATE assessment_session SET session_state = $2 WHERE session_ref = $1",
                &[&"ses_load_created_alpha", &later_state],
            )
            .unwrap();
        let mut transaction = client.transaction().unwrap();
        assert!(
            matches!(
                load_assessment_session(&mut transaction, "ses_load_created_alpha"),
                Err(AssessmentSessionPersistenceError::UnsupportedStoredState)
            ),
            "load must fail closed for stored {later_state} without command history"
        );
        transaction.rollback().unwrap();
    }

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_assessment_session(&mut transaction, "12345"),
        Err(AssessmentSessionPersistenceError::InvalidReference)
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
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::UnsupportedIsolationLevel)
    ));
    assert!(matches!(
        load_assessment_session(&mut transaction, session.session_ref()),
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
    let missing_load = load_assessment_session(&mut transaction, session.session_ref())
        .expect_err("load against a missing table must return the database error");
    assert!(matches!(
        missing_load,
        AssessmentSessionPersistenceError::Database(_)
    ));
    assert_eq!(
        missing_load.to_string(),
        "PostgreSQL assessment-session persistence failed"
    );
    assert!(std::error::Error::source(&missing_load).is_some());
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
             CREATE OR REPLACE FUNCTION assessment_session_redirect_after_insert()
             RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 PERFORM set_config('search_path', '{sink}', false);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER assessment_session_redirect_after_insert
             AFTER INSERT ON assessment_session
             FOR EACH STATEMENT EXECUTE FUNCTION assessment_session_redirect_after_insert();"
        ))
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    let result = persist_assessment_session(&mut transaction, &session);
    transaction.rollback().unwrap();

    client
        .batch_execute(&format!(
            "DROP TRIGGER IF EXISTS assessment_session_redirect_after_insert ON {SCHEMA}.assessment_session;
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

#[test]
fn activated_session_survives_restart_after_command_persist() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut session = created_session(
        "ses_restart_active_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    session
        .apply_command("cmd_activate_after_create", 1, SessionCommand::Activate)
        .unwrap();
    session
        .apply_command("cmd_pause_after_activate", 2, SessionCommand::Pause)
        .unwrap();
    assert_eq!(
        persist_assessment_session_commands(&mut transaction, &session).unwrap(),
        AssessmentSessionPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_assessment_session_commands(&mut transaction, &session).unwrap(),
        AssessmentSessionPersistenceDisposition::Duplicate
    );
    transaction.commit().unwrap();

    let mut load_transaction = client.transaction().unwrap();
    let mut loaded = load_assessment_session(&mut load_transaction, "ses_restart_active_alpha")
        .unwrap()
        .expect("activated session must reload after restart");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.state(), SessionState::Paused);
    assert!(!loaded.state().accepts_responses());
    assert_eq!(loaded.accepted_commands().len(), 2);
    assert_eq!(
        loaded
            .apply_command("cmd_resume_after_restart", 3, SessionCommand::Resume)
            .unwrap(),
        SessionState::Active
    );
    assert!(loaded.state().accepts_responses());
}

#[test]
fn command_persist_requires_created_identity_and_rejects_conflicts() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut session = created_session(
        "ses_command_conflict_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    session
        .apply_command("cmd_activate_conflict", 1, SessionCommand::Activate)
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::MissingCreatedIdentity)
    ));
    transaction.rollback().unwrap();

    let created = created_session(
        "ses_command_conflict_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &created).unwrap();
    persist_assessment_session_commands(&mut transaction, &session).unwrap();
    transaction.commit().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET command_name = $2
             WHERE session_ref = $1 AND command_ref = $3",
            &[
                &"ses_command_conflict_alpha",
                &"pause",
                &"cmd_activate_conflict",
            ],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET command_name = $2
             WHERE session_ref = $1 AND command_ref = $3",
            &[
                &"ses_command_conflict_alpha",
                &"activate",
                &"cmd_activate_conflict",
            ],
        )
        .unwrap();
    client
        .execute(
            "UPDATE assessment_session_command
             SET command_ref = $2
             WHERE session_ref = $1 AND command_sequence = 1",
            &[&"ses_command_conflict_alpha", &"cmd_other_sequence_owner"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::SequenceConflict)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn stale_shorter_command_history_cannot_rewind_paused_projection() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut session = created_session(
        "ses_stale_prefix_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    session
        .apply_command("cmd_activate_before_pause", 1, SessionCommand::Activate)
        .unwrap();
    session
        .apply_command("cmd_pause_after_activate", 2, SessionCommand::Pause)
        .unwrap();
    persist_assessment_session_commands(&mut transaction, &session).unwrap();
    transaction.commit().unwrap();

    let mut stale = created_session(
        "ses_stale_prefix_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    stale
        .apply_command("cmd_activate_before_pause", 1, SessionCommand::Activate)
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &stale),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut load_transaction = client.transaction().unwrap();
    let loaded = load_assessment_session(&mut load_transaction, "ses_stale_prefix_alpha")
        .unwrap()
        .expect("paused session must remain loadable after a rejected stale persist");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.state(), SessionState::Paused);
    assert_eq!(loaded.accepted_commands().len(), 2);
}

#[test]
fn concurrent_stale_shorter_persist_cannot_rewind_paused_projection() {
    let _database_test_guard = test_guard();
    let mut setup_client = connect_client();
    reset_session_table(&mut setup_client);
    apply_assessment_session_migration(&mut setup_client).unwrap();
    let mut session = created_session(
        "ses_stale_prefix_concurrent",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    {
        let mut transaction = setup_client.transaction().unwrap();
        persist_assessment_session(&mut transaction, &session).unwrap();
        session
            .apply_command("cmd_activate_before_pause", 1, SessionCommand::Activate)
            .unwrap();
        persist_assessment_session_commands(&mut transaction, &session).unwrap();
        transaction.commit().unwrap();
    }

    let mut paused = session;
    paused
        .apply_command("cmd_pause_after_activate", 2, SessionCommand::Pause)
        .unwrap();
    let mut stale = created_session(
        "ses_stale_prefix_concurrent",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    stale
        .apply_command("cmd_activate_before_pause", 1, SessionCommand::Activate)
        .unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [paused, stale]
        .into_iter()
        .map(|worker_session| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut client = connect_client();
                barrier.wait();
                let mut transaction = client.transaction().unwrap();
                match persist_assessment_session_commands(&mut transaction, &worker_session) {
                    Ok(_) => {
                        transaction.commit().unwrap();
                        Ok(worker_session.state())
                    }
                    Err(AssessmentSessionPersistenceError::ConflictingReplay) => {
                        transaction.rollback().unwrap();
                        Err(AssessmentSessionPersistenceError::ConflictingReplay)
                    }
                    Err(error) => panic!("unexpected concurrent command persist error: {error:?}"),
                }
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(
        outcomes.iter().any(Result::is_ok),
        "at least the later Pause persist must commit: {outcomes:?}"
    );

    let mut load_transaction = setup_client.transaction().unwrap();
    let loaded = load_assessment_session(&mut load_transaction, "ses_stale_prefix_concurrent")
        .unwrap()
        .expect("paused session must remain loadable after concurrent stale persist");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.state(), SessionState::Paused);
    assert_eq!(loaded.accepted_commands().len(), 2);
}

#[test]
fn command_persist_locks_session_header_until_caller_commits() {
    let (_database_test_guard, mut holder) = test_client();
    reset_session_table(&mut holder);
    apply_assessment_session_migration(&mut holder).unwrap();
    let created = created_session(
        "ses_header_lock_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let mut paused = created_session(
        "ses_header_lock_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    paused
        .apply_command("cmd_activate_before_lock", 1, SessionCommand::Activate)
        .unwrap();
    paused
        .apply_command("cmd_pause_under_lock", 2, SessionCommand::Pause)
        .unwrap();
    let mut stale = created_session(
        "ses_header_lock_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    stale
        .apply_command("cmd_activate_before_lock", 1, SessionCommand::Activate)
        .unwrap();

    {
        let mut created_transaction = holder.transaction().unwrap();
        persist_assessment_session(&mut created_transaction, &created).unwrap();
        created_transaction.commit().unwrap();
    }
    let mut hold_transaction = holder.transaction().unwrap();
    persist_assessment_session_commands(&mut hold_transaction, &paused).unwrap();

    let mut waiter = connect_client();
    waiter
        .batch_execute(&format!(
            "SET search_path TO {SCHEMA}; SET lock_timeout = '200ms';"
        ))
        .unwrap();
    let mut wait_transaction = waiter.transaction().unwrap();
    let error = persist_assessment_session_commands(&mut wait_transaction, &stale)
        .expect_err("a second writer must wait on the locked session header");
    wait_transaction.rollback().unwrap();
    assert!(
        matches!(error, AssessmentSessionPersistenceError::Database(_)),
        "lock timeout must surface as a database failure, not a successful rewind: {error:?}"
    );
    let mut chain = String::new();
    let mut current = Some(&error as &dyn std::error::Error);
    while let Some(item) = current {
        if !chain.is_empty() {
            chain.push_str(" | ");
        }
        let _ = write!(chain, "{item:?}: {item}");
        current = item.source();
    }
    assert!(
        error.to_string() == "PostgreSQL assessment-session persistence failed"
            && (chain.contains("lock timeout") || chain.contains("LockNotAvailable")),
        "the waiter must fail because the header row is locked, not because the prefix check ran on stale committed state: {chain}"
    );

    hold_transaction.commit().unwrap();
    let mut load_transaction = holder.transaction().unwrap();
    let loaded = load_assessment_session(&mut load_transaction, "ses_header_lock_alpha")
        .unwrap()
        .expect("paused session must load after the locking persist commits");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.state(), SessionState::Paused);
    assert_eq!(loaded.accepted_commands().len(), 2);
}

#[test]
fn command_persist_rejects_identity_mismatch_and_resulting_state_rebind() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let created = created_session(
        "ses_command_identity_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    let mut rebound = created_session(
        "ses_command_identity_alpha",
        "ptc_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );
    rebound
        .apply_command("cmd_activate_identity", 1, SessionCommand::Activate)
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &created).unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &rebound),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();

    let mut session = created;
    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    session
        .apply_command("cmd_activate_identity", 1, SessionCommand::Activate)
        .unwrap();
    persist_assessment_session_commands(&mut transaction, &session).unwrap();
    transaction.commit().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET resulting_state = $2
             WHERE session_ref = $1 AND command_ref = $3",
            &[
                &"ses_command_identity_alpha",
                &"paused",
                &"cmd_activate_identity",
            ],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_session_commands(&mut transaction, &session),
        Err(AssessmentSessionPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn load_replays_fail_closed_when_command_or_projection_evidence_is_corrupt() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut session = created_session(
        "ses_load_corrupt_alpha",
        PARTICIPANT_REF,
        "release_big_five_ko_v1",
        VALID_DIGEST,
    );

    let mut transaction = client.transaction().unwrap();
    persist_assessment_session(&mut transaction, &session).unwrap();
    session
        .apply_command("cmd_activate_for_load", 1, SessionCommand::Activate)
        .unwrap();
    persist_assessment_session_commands(&mut transaction, &session).unwrap();
    transaction.commit().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET command_name = $2, resulting_state = $3
             WHERE session_ref = $1 AND command_sequence = 1",
            &[&"ses_load_corrupt_alpha", &"pause", &"paused"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_assessment_session(&mut transaction, "ses_load_corrupt_alpha"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ),
        "Created plus stored Pause must fail closed instead of inventing a lifecycle path"
    );
    transaction.rollback().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET command_name = $2, resulting_state = $3
             WHERE session_ref = $1 AND command_sequence = 1",
            &[&"ses_load_corrupt_alpha", &"activate", &"paused"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_assessment_session(&mut transaction, "ses_load_corrupt_alpha"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ),
        "Activate that stored paused must fail closed"
    );
    transaction.rollback().unwrap();

    client
        .execute(
            "UPDATE assessment_session_command
             SET command_name = $2, resulting_state = $3
             WHERE session_ref = $1 AND command_sequence = 1",
            &[&"ses_load_corrupt_alpha", &"activate", &"active"],
        )
        .unwrap();
    client
        .execute(
            "UPDATE assessment_session SET session_state = $2 WHERE session_ref = $1",
            &[&"ses_load_corrupt_alpha", &"paused"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_assessment_session(&mut transaction, "ses_load_corrupt_alpha"),
            Err(AssessmentSessionPersistenceError::InvalidStoredIdentity)
        ),
        "projection that does not match replayed Active must fail closed"
    );
    transaction.rollback().unwrap();
}

#[test]
fn start_created_session_persists_while_release_is_published() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let release = published_release("release_big_five_ko_v1", VALID_DIGEST);
    let mut transaction = client.transaction().unwrap();
    let (session, disposition) = start_created_assessment_session(
        &mut transaction,
        "ses_start_published_alpha",
        PARTICIPANT_REF,
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        disposition,
        AssessmentSessionPersistenceDisposition::Inserted
    );
    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(session.instrument_release_ref(), "release_big_five_ko_v1");

    let mut load_transaction = client.transaction().unwrap();
    let loaded = load_assessment_session(&mut load_transaction, "ses_start_published_alpha")
        .unwrap()
        .expect("a started session must reload after commit");
    load_transaction.commit().unwrap();
    assert_eq!(loaded.session_ref(), "ses_start_published_alpha");
    assert_eq!(loaded.state(), SessionState::Created);
}

#[test]
fn start_created_session_fails_after_release_is_suspended() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut release = published_release("release_big_five_ko_v1", VALID_DIGEST);
    release
        .apply_command(
            "publication_suspend_start_persist",
            PublicationCommand::Suspend,
            10_300,
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = start_created_assessment_session(
        &mut transaction,
        "ses_start_suspended_alpha",
        PARTICIPANT_REF,
        &release,
        "ko-KR",
        20_000,
    )
    .expect_err("a buyer must not start after the release is suspended");
    transaction.rollback().unwrap();
    assert!(
        matches!(
            error,
            AssessmentSessionStartError::Creation(
                SessionCreationError::InstrumentReleaseUnavailable
            )
        ),
        "start after suspend must fail closed, not persist a reconstituted identity: {error:?}"
    );
}

#[test]
fn start_created_session_replays_after_later_suspend() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut release = published_release("release_big_five_ko_v1", VALID_DIGEST);
    {
        let mut transaction = client.transaction().unwrap();
        start_created_assessment_session(
            &mut transaction,
            "ses_start_replay_alpha",
            PARTICIPANT_REF,
            &release,
            "ko-KR",
            20_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }
    release
        .apply_command(
            "publication_suspend_after_start",
            PublicationCommand::Suspend,
            20_100,
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let (session, disposition) = start_created_assessment_session(
        &mut transaction,
        "ses_start_replay_alpha",
        PARTICIPANT_REF,
        &release,
        "ko-KR",
        20_000,
    )
    .expect("exact start replay must return the original session after later suspend");
    transaction.commit().unwrap();
    assert_eq!(
        disposition,
        AssessmentSessionPersistenceDisposition::Duplicate
    );
    assert_eq!(session.session_ref(), "ses_start_replay_alpha");
    assert_eq!(session.state(), SessionState::Created);
}

#[test]
fn start_created_session_rejects_rebound_identity_after_suspend() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let mut release = published_release("release_big_five_ko_v1", VALID_DIGEST);
    {
        let mut transaction = client.transaction().unwrap();
        start_created_assessment_session(
            &mut transaction,
            "ses_start_rebind_alpha",
            PARTICIPANT_REF,
            &release,
            "ko-KR",
            20_000,
        )
        .unwrap();
        transaction.commit().unwrap();
    }
    release
        .apply_command(
            "publication_suspend_after_rebind",
            PublicationCommand::Suspend,
            20_100,
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = start_created_assessment_session(
        &mut transaction,
        "ses_start_rebind_alpha",
        "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &release,
        "ko-KR",
        20_000,
    )
    .expect_err("a later start must not rebind the stored participant");
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay
        )
    ));
}

#[test]
fn start_created_session_rejects_locale_mismatch() {
    let (_database_test_guard, mut client) = test_client();
    reset_session_table(&mut client);
    apply_assessment_session_migration(&mut client).unwrap();
    let release = published_release("release_big_five_ko_v1", VALID_DIGEST);
    let mut transaction = client.transaction().unwrap();
    let error = start_created_assessment_session(
        &mut transaction,
        "ses_start_locale_alpha",
        PARTICIPANT_REF,
        &release,
        "en-US",
        20_000,
    )
    .expect_err("start must keep the exact published locale");
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        AssessmentSessionStartError::Creation(SessionCreationError::LocaleMismatch)
    ));
}
