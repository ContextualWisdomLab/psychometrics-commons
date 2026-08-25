//! Real `PostgreSQL` acceptance for bounded startable-instrument catalog pages.
//!
//! A public catalog cannot buffer an unbounded number of durable instrument
//! releases. Pagination must preserve the database ordering without duplicates
//! or gaps while leaving final session-start authorization to the locking path.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_instrument_catalog::
    list_startable_instrument_release_page;
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4953_4341_5441_4c47;

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL catalog fixture lock should be acquired");
    client
}

fn test_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS instrument_startable_catalog_pagination_test;\
             SET search_path TO instrument_startable_catalog_pagination_test;",
        )
        .unwrap();
    client
}

fn manifest(release_ref: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_big_five_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "en-US",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_version_big_five_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        RELEASE_DIGEST,
    )
    .unwrap()
}

fn approved_evidence(release_ref: &str) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        format!("publication_evidence_{release_ref}"),
        "evidence_policy_self_reflection_v1",
        release_ref,
        "instrument_version_big_five_v1",
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
        "en-US",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_version_big_five_v1"),
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
        &["recovery_big_five_v1"],
        &["approval_psychometrics_big_five_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn published_release(release_ref: &str) -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(release_ref), 40_000).unwrap();
    release
        .apply_command(
            format!("publication_submit_review_{release_ref}"),
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence(release_ref))
        .unwrap();
    release
        .apply_command(
            format!("publication_publish_{release_ref}"),
            PublicationCommand::Publish,
            40_200,
        )
        .unwrap();
    release
}

fn persist(client: &mut Client, release: &InstrumentRelease) {
    let mut transaction = client.transaction().unwrap();
    persist_instrument_release(&mut transaction, release).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn startable_catalog_paginates_without_duplicates_or_gaps() {
    let _guard = test_guard();
    let mut client = test_client();
    client
        .batch_execute("DROP TABLE IF EXISTS instrument_startable_catalog_pagination_test.instrument_release;")
        .unwrap();
    apply_instrument_release_migration(&mut client).unwrap();

    for index in 0..101 {
        let release_ref = format!("release_big_five_en_{index:03}");
        persist(&mut client, &published_release(&release_ref));
    }

    let mut transaction = client.transaction().unwrap();
    let first = list_startable_instrument_release_page(&mut transaction, None).unwrap();
    assert_eq!(first.releases().len(), 100);
    let cursor = first
        .next_cursor()
        .cloned()
        .expect("a full first page must expose a continuation cursor");
    assert_eq!(
        first.releases().first().unwrap().manifest().release_ref(),
        "release_big_five_en_000"
    );
    assert_eq!(
        first.releases().last().unwrap().manifest().release_ref(),
        "release_big_five_en_099"
    );
    transaction.commit().unwrap();

    let mut transaction = client.transaction().unwrap();
    let second = list_startable_instrument_release_page(&mut transaction, Some(&cursor)).unwrap();
    assert_eq!(second.releases().len(), 1);
    assert_eq!(
        second.releases()[0].manifest().release_ref(),
        "release_big_five_en_100"
    );
    assert!(second.next_cursor().is_none());
    transaction.commit().unwrap();
}