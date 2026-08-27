//! Real `PostgreSQL` contract for bounded family-scoped startable instrument discovery.
//!
//! Family filtering belongs in the product store rather than transport code. The
//! result remains advisory: session start must reload and lock the exact release.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_instrument_catalog::{
    list_startable_instrument_release_page_for_family, StartableInstrumentCatalogError,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release, InstrumentReleaseQueryError,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DATABASE_TEST_LOCK_KEY: i64 = 0x4641_4d43_4154_414c;

fn connect_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn test_guard() -> Client {
    let mut client = connect_client();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&DATABASE_TEST_LOCK_KEY])
        .expect("shared PostgreSQL family-catalog fixture lock should be acquired");
    client
}

fn test_client() -> Client {
    let mut client = connect_client();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS instrument_family_catalog_current_test;\
             SET search_path TO instrument_family_catalog_current_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS instrument_family_catalog_current_test.instrument_release;",
        )
        .unwrap();
}

fn manifest(release_ref: &str, instrument_ref: &str, locale: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        instrument_ref,
        "instrument_version_big_five_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        locale,
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

fn approved_evidence(release_ref: &str, locale: &str) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_v1",
        "evidence_policy_self_reflection_v1",
        release_ref,
        "instrument_version_big_five_v1",
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
        locale,
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

fn published_release(release_ref: &str, instrument_ref: &str, locale: &str) -> InstrumentRelease {
    let mut release =
        InstrumentRelease::new(manifest(release_ref, instrument_ref, locale), 50_000).unwrap();
    release
        .apply_command(
            "publication_submit_review_event",
            PublicationCommand::SubmitReview,
            50_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_evidence(release_ref, locale))
        .unwrap();
    release
        .apply_command(
            "publication_publish_event",
            PublicationCommand::Publish,
            50_200,
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
fn family_page_returns_only_exact_family_in_locale_release_order() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist(
        &mut client,
        &published_release("release_big_five_ko_v2", "instrument_big_five", "ko-KR"),
    );
    persist(
        &mut client,
        &published_release("release_alpha_en_v1", "instrument_alpha", "en-US"),
    );
    persist(
        &mut client,
        &published_release("release_big_five_en_v1", "instrument_big_five", "en-US"),
    );
    persist(
        &mut client,
        &published_release("release_big_five_ko_v1", "instrument_big_five", "ko-KR"),
    );

    let mut transaction = client.transaction().unwrap();
    let page = list_startable_instrument_release_page_for_family(
        &mut transaction,
        "instrument_big_five",
        None,
    )
    .unwrap();
    let identities: Vec<(&str, &str, &str)> = page
        .releases()
        .iter()
        .map(|release| {
            (
                release.manifest().instrument_ref(),
                release.manifest().locale(),
                release.manifest().release_ref(),
            )
        })
        .collect();
    assert_eq!(
        identities,
        [
            ("instrument_big_five", "en-US", "release_big_five_en_v1"),
            ("instrument_big_five", "ko-KR", "release_big_five_ko_v1"),
            ("instrument_big_five", "ko-KR", "release_big_five_ko_v2"),
        ]
    );
    assert!(page.next_cursor().is_none());
    transaction.commit().unwrap();
}

#[test]
fn family_page_returns_empty_for_valid_unknown_family() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();
    persist(
        &mut client,
        &published_release("release_big_five_en_v1", "instrument_big_five", "en-US"),
    );

    let mut transaction = client.transaction().unwrap();
    let page = list_startable_instrument_release_page_for_family(
        &mut transaction,
        "instrument_unknown",
        None,
    )
    .unwrap();
    assert!(page.releases().is_empty());
    assert!(page.next_cursor().is_none());
    transaction.commit().unwrap();
}

#[test]
fn family_page_rejects_blank_numeric_and_noncanonical_family_refs() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    for invalid in ["", " ", "123", " instrument_big_five", "instrument_big_five "] {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            list_startable_instrument_release_page_for_family(&mut transaction, invalid, None),
            Err(StartableInstrumentCatalogError::Query(
                InstrumentReleaseQueryError::InvalidReference
            ))
        ));
        transaction.rollback().unwrap();
    }
}
