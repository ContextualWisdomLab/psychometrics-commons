//! Concurrency contract for instrument-release persist classification.
//!
//! A Duplicate published classification must keep the `instrument_release` row
//! locked until the caller transaction ends. Otherwise a second writer can
//! Suspend or Retire after the first writer already decided the release is
//! still published, and a later start in that same transaction would insert
//! against a store that no longer accepts new sessions.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
    InstrumentReleasePersistenceDisposition,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn schema_name(prefix: &str) -> String {
    format!("{prefix}_{}", std::process::id())
}

fn connect() -> Client {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&url, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn ready_client(prefix: &str) -> Client {
    let mut client = connect();
    let schema = schema_name(prefix);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    apply_instrument_release_migration(&mut client).unwrap();
    client
}

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
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
        RELEASE_DIGEST,
    )
    .unwrap()
}

fn approved_publication_evidence() -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
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
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 40_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_publication_evidence())
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 40_200)
        .unwrap();
    release
}

fn suspended_release() -> InstrumentRelease {
    let mut release = published_release();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 40_300)
        .unwrap();
    release
}

#[test]
fn duplicate_published_classification_holds_row_lock_until_transaction_end() {
    let prefix = "instrument_release_publication_lock";
    let mut client = ready_client(prefix);
    let published = published_release();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_instrument_release(&mut transaction, &published).unwrap(),
            InstrumentReleasePersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let mut classifier = client.transaction().unwrap();
    assert_eq!(
        persist_instrument_release(&mut classifier, &published).unwrap(),
        InstrumentReleasePersistenceDisposition::Duplicate
    );

    let mut contender = connect();
    let schema = schema_name(prefix);
    contender
        .batch_execute(&format!(
            "SET search_path TO {schema}; SET lock_timeout TO '100ms';"
        ))
        .unwrap();
    let mut contender_transaction = contender.transaction().unwrap();
    let error = persist_instrument_release(&mut contender_transaction, &suspended_release())
        .expect_err(
            "duplicate published classification must keep the publication row locked so a concurrent suspend cannot hide from the open transaction",
        );
    assert!(
        matches!(
            error,
            psychometrics_commons_runtime::postgres_instrument_release::InstrumentReleasePersistenceError::Database(_)
        ),
        "the waiter must fail as a database lock timeout, not as a successful suspend: {error:?}"
    );
    let source =
        std::error::Error::source(&error).expect("database failure must keep the postgres source");
    assert!(
        source.to_string().contains("lock timeout"),
        "the waiter must fail because the publication row is locked: {source}"
    );
    contender_transaction.rollback().unwrap();
    classifier.rollback().unwrap();
}
