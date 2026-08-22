//! Stored-release retries preserve exact session and participant reference spelling.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_assessment_session::{
    apply_assessment_session_migration, start_created_assessment_session,
    start_created_assessment_session_from_stored_release, AssessmentSessionPersistenceDisposition,
    AssessmentSessionStartError,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
};
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "session_replay_exact_reference_test";
const PARTICIPANT_REF: &str = "participant_replay_exact_alpha";
const RELEASE_REF: &str = "release_replay_exact_alpha";
const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
static DATABASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn client() -> (MutexGuard<'static, ()>, Client) {
    let guard = DATABASE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; \
             CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
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
        DIGEST,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "publication_review_replay_exact",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_replay_exact",
                "evidence_policy_self_reflection_v1",
                RELEASE_REF,
                "instrument_version_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
                DIGEST,
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
            "publication_publish_replay_exact",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn seed_started_session(client: &mut Client, session_ref: &str) -> InstrumentRelease {
    let release = published_release();
    let mut transaction = client.transaction().unwrap();
    persist_instrument_release(&mut transaction, &release).unwrap();
    let (_, disposition) = start_created_assessment_session(
        &mut transaction,
        session_ref,
        PARTICIPANT_REF,
        &release,
        "ko-KR",
        20_000,
    )
    .unwrap();
    assert_eq!(
        disposition,
        AssessmentSessionPersistenceDisposition::Inserted
    );
    transaction.commit().unwrap();
    release
}

fn assert_padded_stored_replays_fail(client: &mut Client, session_ref: &str) {
    for padded_session_ref in [
        format!(" {session_ref}"),
        format!("\u{00a0}{session_ref}"),
        format!("{session_ref}\u{3000}"),
    ] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                &padded_session_ref,
                PARTICIPANT_REF,
                RELEASE_REF,
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
        transaction.rollback().unwrap();
    }

    for padded_participant_ref in [
        format!(" {PARTICIPANT_REF}"),
        format!("\u{202f}{PARTICIPANT_REF}"),
        format!("{PARTICIPANT_REF}\u{2003}"),
    ] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            start_created_assessment_session_from_stored_release(
                &mut transaction,
                session_ref,
                &padded_participant_ref,
                RELEASE_REF,
                "ko-KR",
                20_000,
            ),
            Err(AssessmentSessionStartError::InvalidReference)
        ));
        transaction.rollback().unwrap();
    }
}

#[test]
fn suspended_stored_release_replay_rejects_padded_resource_aliases() {
    let (_guard, mut client) = client();
    let session_ref = "session_replay_exact_suspended";
    let _release = seed_started_session(&mut client, session_ref);
    client
        .execute(
            "UPDATE instrument_release SET publication_state = 'suspended' WHERE release_ref = $1",
            &[&RELEASE_REF],
        )
        .unwrap();

    assert_padded_stored_replays_fail(&mut client, session_ref);

    let mut transaction = client.transaction().unwrap();
    assert_eq!(
        start_created_assessment_session_from_stored_release(
            &mut transaction,
            session_ref,
            PARTICIPANT_REF,
            RELEASE_REF,
            "ko-KR",
            20_000,
        )
        .unwrap()
        .1,
        AssessmentSessionPersistenceDisposition::Duplicate
    );
    transaction.rollback().unwrap();
}

#[test]
fn retired_stored_release_replay_rejects_padded_resource_aliases() {
    let (_guard, mut client) = client();
    let session_ref = "session_replay_exact_retired";
    let _release = seed_started_session(&mut client, session_ref);
    client
        .execute(
            "UPDATE instrument_release SET publication_state = 'retired' WHERE release_ref = $1",
            &[&RELEASE_REF],
        )
        .unwrap();

    assert_padded_stored_replays_fail(&mut client, session_ref);
}
