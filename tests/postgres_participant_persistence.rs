//! Real `PostgreSQL` contract for durable anonymous assessment participants.
//!
//! A transport must persist the participant row, then load it by tenant and
//! participant reference, before anonymous command authorization. Reconstructing
//! the participant from the proof would make the tenant check tautological.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::anonymous_authorization::authorize_anonymous_session_command;
use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_participant::{
    apply_assessment_participant_migration, load_assessment_participant,
    persist_assessment_participant, ParticipantPersistenceDisposition, ParticipantPersistenceError,
};
use psychometrics_commons_runtime::session::AssessmentSession;
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

static PARTICIPANT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn participant_test_guard() -> MutexGuard<'static, ()> {
    PARTICIPANT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS assessment_participant_persistence_test;\
             SET search_path TO assessment_participant_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_participant_table(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS assessment_participant_persistence_test.assessment_participant;",
        )
        .unwrap();
}

fn persist_ok(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> ParticipantPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_assessment_participant(&mut transaction, participant).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    participant: &ParticipantRecord,
) -> ParticipantPersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_assessment_participant(&mut transaction, participant).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap();
    let evidence = PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001"],
        RELEASE_DIGEST,
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
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_11d5b1e7",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release.bind_publication_evidence(evidence).unwrap();
    release
        .apply_command(
            "publication_publish_20f6c2a8",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

#[test]
fn anonymous_participant_persist_is_exactly_idempotent_and_reloadable() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let participant =
        ParticipantRecord::new_anonymous("participant_persist_alpha", "tenant_alpha", 1_000)
            .unwrap();
    assert_eq!(
        persist_ok(&mut client, &participant),
        ParticipantPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_ok(&mut client, &participant),
        ParticipantPersistenceDisposition::Duplicate
    );

    let loaded =
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_alpha")
            .unwrap();
    assert_eq!(loaded.participant_ref(), "participant_persist_alpha");
    assert_eq!(loaded.tenant_ref(), "tenant_alpha");
    assert_eq!(loaded.created_at_unix_ms(), 1_000);
    assert!(loaded.linked_subject_ref().is_none());
}

#[test]
fn conflicting_tenant_or_creation_time_replay_fails_closed() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let original =
        ParticipantRecord::new_anonymous("participant_persist_beta", "tenant_alpha", 2_000)
            .unwrap();
    persist_ok(&mut client, &original);

    let foreign_tenant =
        ParticipantRecord::new_anonymous("participant_persist_beta", "tenant_beta", 2_000).unwrap();
    assert!(matches!(
        persist_err(&mut client, &foreign_tenant),
        ParticipantPersistenceError::ConflictingReplay
    ));

    let moved_clock =
        ParticipantRecord::new_anonymous("participant_persist_beta", "tenant_alpha", 3_000)
            .unwrap();
    assert!(matches!(
        persist_err(&mut client, &moved_clock),
        ParticipantPersistenceError::ConflictingReplay
    ));
}

#[test]
fn load_requires_the_stored_tenant_and_does_not_leak_a_foreign_row() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let participant =
        ParticipantRecord::new_anonymous("participant_persist_gamma", "tenant_alpha", 4_000)
            .unwrap();
    persist_ok(&mut client, &participant);

    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_beta", "participant_persist_gamma"),
        Err(ParticipantPersistenceError::NotFound)
    ));
    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "participant_missing"),
        Err(ParticipantPersistenceError::NotFound)
    ));
}

#[test]
fn loaded_persisted_participant_authorizes_only_its_stored_tenant() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let stored =
        ParticipantRecord::new_anonymous("participant_persist_delta", "tenant_alpha", 5_000)
            .unwrap();
    persist_ok(&mut client, &stored);
    let loaded =
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_delta")
            .unwrap();
    let session = AssessmentSession::new(
        "session_persist_delta",
        loaded.participant_ref(),
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap();
    let actor = AnonymousSessionContext::new(
        "tenant_alpha",
        "participant_persist_delta",
        "session_persist_delta",
        "anonymous_persist_evidence_delta",
        6_000,
    )
    .unwrap();

    assert_eq!(
        authorize_anonymous_session_command(&actor, &loaded, &session, 5_500),
        Ok(())
    );
    assert!(matches!(
        load_assessment_participant(&mut client, actor.tenant_ref(), "participant_other"),
        Err(ParticipantPersistenceError::NotFound)
    ));
}

#[test]
fn linked_participant_persistence_is_out_of_scope_for_this_slice() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let mut linked =
        ParticipantRecord::new_anonymous("participant_persist_epsilon", "tenant_alpha", 7_000)
            .unwrap();
    linked
        .link_account(
            "link_event_persist_epsilon",
            "issuer_keyverse",
            "subject_epsilon",
            "anonymous_proof_epsilon",
            "authenticated_proof_epsilon",
            7_100,
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &linked),
        ParticipantPersistenceError::IdentityLinkOutOfScope
    ));
    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_epsilon"),
        Err(ParticipantPersistenceError::NotFound)
    ));
}

#[test]
fn overflow_timestamp_and_invalid_load_references_fail_closed() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let overflow =
        ParticipantRecord::new_anonymous("participant_persist_zeta", "tenant_alpha", u64::MAX)
            .unwrap();
    assert!(matches!(
        persist_err(&mut client, &overflow),
        ParticipantPersistenceError::InvalidTimestamp
    ));
    assert!(matches!(
        load_assessment_participant(&mut client, " ", "participant_persist_zeta"),
        Err(ParticipantPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "12"),
        Err(ParticipantPersistenceError::InvalidReference)
    ));
}

#[test]
fn assessment_participant_persistence_requires_read_committed() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();

    let participant =
        ParticipantRecord::new_anonymous("participant_persist_eta", "tenant_alpha", 8_000).unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_assessment_participant(&mut transaction, &participant),
        Err(ParticipantPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn stored_status_mismatch_is_conflicting_replay() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();
    let participant =
        ParticipantRecord::new_anonymous("participant_persist_status", "tenant_alpha", 8_500)
            .unwrap();
    persist_ok(&mut client, &participant);
    client
        .batch_execute(
            "DROP TRIGGER IF EXISTS assessment_participant_immutable_guard \
                 ON assessment_participant;\
             ALTER TABLE assessment_participant \
                 DROP CONSTRAINT assessment_participant_status_value_check;\
             UPDATE assessment_participant SET participant_status = 'corrupted' \
                 WHERE participant_ref = 'participant_persist_status';",
        )
        .unwrap();
    assert!(matches!(
        persist_err(&mut client, &participant),
        ParticipantPersistenceError::ConflictingReplay
    ));
}

#[test]
fn corrupt_created_at_values_fail_closed_on_load() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);
    apply_assessment_participant_migration(&mut client).unwrap();
    client
        .batch_execute(
            "DROP TRIGGER IF EXISTS assessment_participant_immutable_guard \
                 ON assessment_participant;\
             DROP TRIGGER IF EXISTS assessment_participant_truncate_guard \
                 ON assessment_participant;\
             ALTER TABLE assessment_participant \
                 DROP CONSTRAINT assessment_participant_created_at_unix_positive_check;",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_persist_negative', 'tenant_alpha', 'anonymous', -1)",
            &[],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO assessment_participant (\
                 participant_ref, tenant_ref, participant_status, created_at_unix_ms\
             ) VALUES ('participant_persist_zero', 'tenant_alpha', 'anonymous', 0)",
            &[],
        )
        .unwrap();

    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_negative"),
        Err(ParticipantPersistenceError::InvalidTimestamp)
    ));
    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_zero"),
        Err(ParticipantPersistenceError::InvalidTimestamp)
    ));
}

#[test]
fn missing_assessment_participant_relation_is_a_database_failure() {
    let _guard = participant_test_guard();
    let mut client = test_client();
    reset_participant_table(&mut client);

    let participant =
        ParticipantRecord::new_anonymous("participant_persist_theta", "tenant_alpha", 9_000)
            .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_assessment_participant(&mut transaction, &participant),
        Err(ParticipantPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
    assert!(matches!(
        load_assessment_participant(&mut client, "tenant_alpha", "participant_persist_theta"),
        Err(ParticipantPersistenceError::Database(_))
    ));
}
