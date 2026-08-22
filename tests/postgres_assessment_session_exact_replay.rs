//! Exact-reference regression for retries after publication closes.
//!
//! A previously accepted session may be replayed after its release is suspended
//! or retired, but only when the caller presents the exact originally-issued
//! session, participant, and release references. Padded aliases must never reopen it.

use std::sync::{Mutex, MutexGuard};

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::{
    apply_assessment_session_migration, start_created_assessment_session,
    start_created_assessment_session_from_stored_release, AssessmentSessionStartError,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const PARTICIPANT_REF: &str = "ptc_exact_replay_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE_REF: &str = "release_big_five_exact_replay_v1";
const SCHEMA: &str = "assessment_session_exact_replay_test";
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
            "CREATE SCHEMA IF NOT EXISTS {SCHEMA};
             SET search_path TO {SCHEMA};
             DROP TABLE IF EXISTS assessment_session_command;
             DROP TABLE IF EXISTS assessment_session;
             DROP TABLE IF EXISTS instrument_release;"
        ))
        .unwrap();
    apply_instrument_release_migration(&mut client).unwrap();
    apply_assessment_session_migration(&mut client).unwrap();
    (guard, client)
}

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        RELEASE_REF,
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
        VALID_DIGEST,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_exact_replay",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_exact_replay",
                "evidence_policy_self_reflection_v1",
                RELEASE_REF,
                "instrument_version_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
                VALID_DIGEST,
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
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_exact_replay",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn assert_invalid_reference<T>(result: Result<T, AssessmentSessionStartError>) {
    assert!(matches!(
        result.err(),
        Some(AssessmentSessionStartError::InvalidReference)
    ));
}

#[test]
fn closed_publication_replay_rejects_padded_session_participant_and_release_aliases() {
    let (_database_test_guard, mut client) = test_client();
    let published = published_release();
    let mut transaction = client.transaction().unwrap();
    persist_instrument_release(&mut transaction, &published).unwrap();
    for (session_ref, created_at_unix_ms) in [
        ("ses_exact_replay_suspended", 20_000),
        ("ses_exact_replay_retired", 21_000),
    ] {
        start_created_assessment_session(
            &mut transaction,
            session_ref,
            PARTICIPANT_REF,
            &published,
            "ko-KR",
            created_at_unix_ms,
        )
        .unwrap();
    }
    transaction.commit().unwrap();

    for (state, session_ref, created_at_unix_ms) in [
        ("suspended", "ses_exact_replay_suspended", 20_000),
        ("retired", "ses_exact_replay_retired", 21_000),
    ] {
        client
            .execute(
                "UPDATE instrument_release SET publication_state = $2 WHERE release_ref = $1",
                &[&RELEASE_REF, &state],
            )
            .unwrap();

        let padded_session = format!(" {session_ref}");
        let padded_participant = format!("{PARTICIPANT_REF} ");
        let padded_release = format!(" {RELEASE_REF}");
        let mut transaction = client.transaction().unwrap();

        assert_invalid_reference(start_created_assessment_session(
            &mut transaction,
            &padded_session,
            PARTICIPANT_REF,
            &published,
            "ko-KR",
            created_at_unix_ms,
        ));
        assert_invalid_reference(start_created_assessment_session(
            &mut transaction,
            session_ref,
            &padded_participant,
            &published,
            "ko-KR",
            created_at_unix_ms,
        ));
        assert_invalid_reference(start_created_assessment_session_from_stored_release(
            &mut transaction,
            &padded_session,
            PARTICIPANT_REF,
            RELEASE_REF,
            "ko-KR",
            created_at_unix_ms,
        ));
        assert_invalid_reference(start_created_assessment_session_from_stored_release(
            &mut transaction,
            session_ref,
            &padded_participant,
            RELEASE_REF,
            "ko-KR",
            created_at_unix_ms,
        ));
        assert_invalid_reference(start_created_assessment_session_from_stored_release(
            &mut transaction,
            session_ref,
            PARTICIPANT_REF,
            &padded_release,
            "ko-KR",
            created_at_unix_ms,
        ));

        transaction.rollback().unwrap();
    }
}
