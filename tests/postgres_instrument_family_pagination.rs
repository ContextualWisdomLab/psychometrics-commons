//! Real `PostgreSQL` contract for bounded family-scoped catalog pagination.
//!
//! A family with more than one page must remain discoverable without loading the
//! whole published catalog into memory. Continuation state is product-issued and
//! bound to the exact instrument family so a cursor from one family cannot be
//! replayed against another family and silently skip release candidates.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::postgres_instrument_catalog::{
    list_startable_instrument_release_family_page, StartableInstrumentCatalogError,
    STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, persist_instrument_release, InstrumentReleaseQueryError,
};
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

static FAMILY_PAGINATION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    FAMILY_PAGINATION_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS instrument_family_pagination_test;\
             SET search_path TO instrument_family_pagination_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS instrument_family_pagination_test.instrument_release;",
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
fn family_pages_continue_without_gaps_and_reject_cross_family_cursor_replay() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    for index in 0..STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE + 2 {
        let release_ref = format!("release_big_five_{index:03}_v1");
        persist(
            &mut client,
            &published_release(&release_ref, "instrument_big_five", "en-US"),
        );
    }
    persist(
        &mut client,
        &published_release("release_alpha_000_v1", "instrument_alpha", "en-US"),
    );

    let mut transaction = client.transaction().unwrap();
    let first = list_startable_instrument_release_family_page(
        &mut transaction,
        "instrument_big_five",
        None,
    )
    .unwrap();
    assert_eq!(
        first.releases().len(),
        STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE
    );
    let cursor = first.next_cursor().expect("101st row requires continuation");

    assert!(matches!(
        list_startable_instrument_release_family_page(
            &mut transaction,
            "instrument_alpha",
            Some(cursor),
        ),
        Err(StartableInstrumentCatalogError::Query(
            InstrumentReleaseQueryError::InvalidReference
        ))
    ));

    let second = list_startable_instrument_release_family_page(
        &mut transaction,
        "instrument_big_five",
        Some(cursor),
    )
    .unwrap();
    assert_eq!(second.releases().len(), 2);
    assert!(second.next_cursor().is_none());

    let release_refs: Vec<&str> = first
        .releases()
        .iter()
        .chain(second.releases())
        .map(|release| release.manifest().release_ref())
        .collect();
    let expected: Vec<String> = (0..STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE + 2)
        .map(|index| format!("release_big_five_{index:03}_v1"))
        .collect();
    assert_eq!(
        release_refs,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );

    transaction.commit().unwrap();
}
