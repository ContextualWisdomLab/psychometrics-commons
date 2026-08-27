//! Pagination and cursor-family binding for family-scoped durable catalog discovery.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_instrument_catalog::{
    list_startable_instrument_release_page_for_family, StartableInstrumentCatalogError,
    STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release, InstrumentReleaseQueryError,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4641_4d50_4147_4552;

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL family-pagination fixture lock should be acquired");
    client
}

fn test_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS instrument_family_catalog_pagination_test;\
             SET search_path TO instrument_family_catalog_pagination_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS instrument_family_catalog_pagination_test.instrument_release;",
        )
        .unwrap();
}

fn manifest(release_ref: &str, instrument_ref: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        instrument_ref,
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
        "publication_evidence_big_five_v1",
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

fn published_release(release_ref: &str, instrument_ref: &str) -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(release_ref, instrument_ref), 60_000).unwrap();
    release
        .apply_command(
            "publication_submit_review_event",
            PublicationCommand::SubmitReview,
            60_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence(release_ref))
        .unwrap();
    release
        .apply_command(
            "publication_publish_event",
            PublicationCommand::Publish,
            60_200,
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
fn family_cursor_continues_exact_family_and_rejects_cross_family_reuse() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    for index in 0..=STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE {
        let release_ref = format!("release_alpha_{index:03}");
        persist(
            &mut client,
            &published_release(&release_ref, "instrument_alpha"),
        );
    }
    persist(
        &mut client,
        &published_release("release_big_five_000", "instrument_big_five"),
    );

    let mut transaction = client.transaction().unwrap();
    let first = list_startable_instrument_release_page_for_family(
        &mut transaction,
        "instrument_alpha",
        None,
    )
    .unwrap();
    assert_eq!(first.releases().len(), STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE);
    let cursor = first
        .next_cursor()
        .cloned()
        .expect("a family with page size plus one releases must expose continuation");

    let second = list_startable_instrument_release_page_for_family(
        &mut transaction,
        "instrument_alpha",
        Some(&cursor),
    )
    .unwrap();
    assert_eq!(second.releases().len(), 1);
    assert_eq!(
        second.releases()[0].manifest().release_ref(),
        "release_alpha_100"
    );
    assert!(second.next_cursor().is_none());

    assert!(matches!(
        list_startable_instrument_release_page_for_family(
            &mut transaction,
            "instrument_big_five",
            Some(&cursor),
        ),
        Err(StartableInstrumentCatalogError::Query(
            InstrumentReleaseQueryError::InvalidReference
        ))
    ));
    transaction.commit().unwrap();
}
