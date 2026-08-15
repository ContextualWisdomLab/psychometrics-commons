//! Real `PostgreSQL` contract for loading session-eligible instrument releases.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, load_published_instrument_release,
    InstrumentReleaseQueryError,
};
use std::sync::atomic::{AtomicU64, Ordering};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
static SCHEMA_NONCE: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    client: Client,
    schema_name: String,
}

impl TestDatabase {
    fn new() -> Self {
        let connection = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
        let mut client = Client::connect(&connection, NoTls)
            .expect("isolated CI PostgreSQL database must be reachable");
        let schema_name = format!(
            "instrument_release_query_{}_{}",
            std::process::id(),
            SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed)
        );
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {schema_name}; SET search_path TO {schema_name};"
            ))
            .unwrap();
        apply_instrument_release_migration(&mut client).unwrap();
        Self {
            client,
            schema_name,
        }
    }

    fn insert_release(&mut self, release_ref: &str, locale: &str, state: &str) {
        self.client
            .execute(
                "INSERT INTO instrument_release (\
                     release_ref, instrument_ref, instrument_version_ref, construct_ref,\
                     item_version_refs, locale, assessment_spec_ref, scoring_version_ref,\
                     calibration_reference, norm_version_ref, narrative_version_ref,\
                     consent_requirement_refs, intended_use_ref, limitations_ref,\
                     content_digest, publication_state, created_at_unix_ms\
                 ) VALUES ($1,'instrument_big_five','instrument_version_big_five_v1',\
                           'construct_big_five',$2,$3,'assessment_spec_big_five_v1',\
                           'scoring_version_big_five_v1','calibration_big_five_v1',\
                           'norm_version_big_five_v1','narrative_version_big_five_v1',$4,\
                           'intended_use_self_reflection_v1','limitations_nonclinical_v1',\
                           $5,$6,40000)",
                &[
                    &release_ref,
                    &vec!["item_version_001", "item_version_002"],
                    &locale,
                    &vec!["consent_service_v1"],
                    &RELEASE_DIGEST,
                    &state,
                ],
            )
            .unwrap();
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = self.client.batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA IF EXISTS {} CASCADE;",
            self.schema_name
        ));
    }
}

#[test]
fn published_release_query_returns_exact_session_provenance() {
    let mut database = TestDatabase::new();
    database.insert_release("release_big_five_en_v1", "en-US", "published");

    let loaded = load_published_instrument_release(
        &mut database.client,
        "release_big_five_en_v1",
        "en-US",
    )
    .unwrap();

    assert_eq!(loaded.manifest().release_ref(), "release_big_five_en_v1");
    assert_eq!(loaded.manifest().locale(), "en-US");
    assert_eq!(
        loaded.manifest().item_version_refs(),
        &["item_version_001".to_owned(), "item_version_002".to_owned()]
    );
    assert_eq!(loaded.manifest().content_digest(), RELEASE_DIGEST);
    assert_eq!(loaded.created_at_unix_ms(), 40_000);
}

#[test]
fn release_query_fails_closed_for_locale_state_identity_and_missing_release() {
    let mut database = TestDatabase::new();
    database.insert_release("release_big_five_ko_v1", "ko-KR", "published");
    database.insert_release("release_big_five_review_v1", "en-US", "review");

    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            "release_big_five_ko_v1",
            "en-US"
        ),
        Err(InstrumentReleaseQueryError::LocaleMismatch)
    ));
    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            "release_big_five_review_v1",
            "en-US"
        ),
        Err(InstrumentReleaseQueryError::NotPublished)
    ));
    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            " release_big_five_ko_v1",
            "ko-KR"
        ),
        Err(InstrumentReleaseQueryError::InvalidReference)
    ));
    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            "release_missing_big_five",
            "en-US"
        ),
        Err(InstrumentReleaseQueryError::NotFound)
    ));
    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            "release_big_five_ko_v1",
            " en-US"
        ),
        Err(InstrumentReleaseQueryError::InvalidLocale)
    ));
}

#[test]
fn release_query_revalidates_stored_manifest_instead_of_trusting_rows() {
    let mut database = TestDatabase::new();
    database.insert_release("release_big_five_tampered_v1", "en-US", "published");
    database
        .client
        .batch_execute(
            "ALTER TABLE instrument_release DROP CONSTRAINT instrument_release_item_refs_not_empty_check;\
             UPDATE instrument_release SET item_version_refs = ARRAY[]::TEXT[]\
             WHERE release_ref = 'release_big_five_tampered_v1';",
        )
        .unwrap();

    assert!(matches!(
        load_published_instrument_release(
            &mut database.client,
            "release_big_five_tampered_v1",
            "en-US"
        ),
        Err(InstrumentReleaseQueryError::InvalidStoredValue)
    ));
}
