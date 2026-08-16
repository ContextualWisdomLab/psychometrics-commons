//! Real `PostgreSQL` contract for durable instrument-release publication evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    PublicationState,
};
use psychometrics_commons_runtime::postgres_instrument_release::{
    apply_instrument_release_migration, list_startable_instrument_releases,
    load_instrument_release, persist_instrument_release, InstrumentReleasePersistenceDisposition,
    InstrumentReleasePersistenceError,
};
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

static INSTRUMENT_RELEASE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn instrument_release_test_guard() -> MutexGuard<'static, ()> {
    INSTRUMENT_RELEASE_TEST_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS instrument_release_persistence_test;\
             SET search_path TO instrument_release_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_instrument_release_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS instrument_release_persistence_test.instrument_release;",
        )
        .unwrap();
}

fn manifest(release_ref: &str, digest: &str) -> InstrumentReleaseManifest {
    manifest_with_norm(release_ref, digest, Some("norm_version_big_five_ko_v1"))
}

fn manifest_with_norm(
    release_ref: &str,
    digest: &str,
    norm_version_ref: Option<&str>,
) -> InstrumentReleaseManifest {
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
        norm_version_ref,
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        digest,
    )
    .unwrap()
}

fn approved_publication_evidence_for(release_ref: &str) -> PublicationEvidenceRecord {
    approved_publication_evidence_for_locale(release_ref, "ko-KR")
}

fn approved_publication_evidence_for_locale(
    release_ref: &str,
    locale: &str,
) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        release_ref,
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
        locale,
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
    published_release_named("release_big_five_ko_v1")
}

fn published_release_named(release_ref: &str) -> InstrumentRelease {
    published_release_for(release_ref, "instrument_big_five", "ko-KR")
}

fn published_release_for(
    release_ref: &str,
    instrument_ref: &str,
    locale: &str,
) -> InstrumentRelease {
    let mut release = InstrumentRelease::new(
        InstrumentReleaseManifest::new(
            release_ref,
            instrument_ref,
            "instrument_version_big_five_ko_v1",
            "construct_big_five",
            &["item_version_001", "item_version_002"],
            locale,
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
        .unwrap(),
        40_000,
    )
    .unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(approved_publication_evidence_for_locale(
            release_ref,
            locale,
        ))
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 40_200)
        .unwrap();
    assert_eq!(release.state(), PublicationState::Published);
    assert!(release.accepts_new_sessions());
    release
}

fn review_release_named(release_ref: &str) -> InstrumentRelease {
    let mut release =
        InstrumentRelease::new(manifest(release_ref, RELEASE_DIGEST), 40_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();
    release
}

fn suspended_release_named(release_ref: &str) -> InstrumentRelease {
    let mut release = published_release_named(release_ref);
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 40_300)
        .unwrap();
    release
}

fn retired_release_named(release_ref: &str) -> InstrumentRelease {
    let mut release = published_release_named(release_ref);
    release
        .apply_command("retire_event", PublicationCommand::Retire, 40_300)
        .unwrap();
    release
}

fn persist_ok(
    client: &mut Client,
    release: &InstrumentRelease,
) -> InstrumentReleasePersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_instrument_release(&mut transaction, release).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn persist_err(
    client: &mut Client,
    release: &InstrumentRelease,
) -> InstrumentReleasePersistenceError {
    let mut transaction = client.transaction().unwrap();
    let error = persist_instrument_release(&mut transaction, release).unwrap_err();
    transaction.rollback().unwrap();
    error
}

fn stored_state(client: &mut Client, release_ref: &str) -> String {
    client
        .query_one(
            "SELECT publication_state FROM instrument_release WHERE release_ref = $1",
            &[&release_ref],
        )
        .unwrap()
        .get(0)
}

#[test]
fn draft_release_persist_is_exactly_idempotent_and_digest_rebinding_fails_closed() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_instrument_release(&mut transaction, &draft).unwrap(),
            InstrumentReleasePersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_instrument_release(&mut transaction, &draft).unwrap(),
            InstrumentReleasePersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let rebound =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", OTHER_DIGEST), 40_000).unwrap();
    assert!(matches!(
        persist_err(&mut client, &rebound),
        InstrumentReleasePersistenceError::ConflictingReplay
    ));

    let timestamp_rebound =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_001).unwrap();
    assert!(matches!(
        persist_err(&mut client, &timestamp_rebound),
        InstrumentReleasePersistenceError::ConflictingReplay
    ));
}

#[test]
fn published_release_state_advances_without_rewriting_immutable_manifest() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        persist_instrument_release(&mut transaction, &draft).unwrap();
        transaction.commit().unwrap();
    }

    let published = published_release();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_instrument_release(&mut transaction, &published).unwrap(),
            InstrumentReleasePersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let row = client
        .query_one(
            "SELECT publication_state FROM instrument_release WHERE release_ref = $1",
            &[&"release_big_five_ko_v1"],
        )
        .unwrap();
    let stored_state: String = row.get(0);
    assert_eq!(stored_state, "published");

    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_instrument_release(&mut transaction, &published).unwrap(),
            InstrumentReleasePersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }
}

#[test]
fn instrument_release_persistence_requires_read_committed() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_serializable", RELEASE_DIGEST), 40_000).unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_instrument_release(&mut transaction, &draft),
        Err(InstrumentReleasePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_instrument_release_relation_is_a_database_failure() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);

    let draft =
        InstrumentRelease::new(manifest("release_missing", RELEASE_DIGEST), 40_000).unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_instrument_release(&mut transaction, &draft),
        Err(InstrumentReleasePersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn publication_states_and_isolated_releases_persist_independently() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    assert_eq!(
        persist_ok(&mut client, &draft),
        InstrumentReleasePersistenceDisposition::Inserted
    );

    let mut review = draft.clone();
    review
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            40_100,
        )
        .unwrap();
    assert_eq!(
        persist_ok(&mut client, &review),
        InstrumentReleasePersistenceDisposition::Inserted
    );
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "review"
    );

    review
        .bind_publication_evidence(approved_publication_evidence_for("release_big_five_ko_v1"))
        .unwrap();
    review
        .apply_command("publish_event", PublicationCommand::Publish, 40_200)
        .unwrap();
    persist_ok(&mut client, &review);
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "published"
    );

    review
        .apply_command("suspend_event", PublicationCommand::Suspend, 40_300)
        .unwrap();
    persist_ok(&mut client, &review);
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "suspended"
    );

    review
        .apply_command("retire_event", PublicationCommand::Retire, 40_400)
        .unwrap();
    persist_ok(&mut client, &review);
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "retired"
    );

    let neighbor =
        InstrumentRelease::new(manifest("release_neighbor_en_v1", OTHER_DIGEST), 50_000).unwrap();
    assert_eq!(
        persist_ok(&mut client, &neighbor),
        InstrumentReleasePersistenceDisposition::Inserted
    );
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "retired"
    );
    assert_eq!(stored_state(&mut client, "release_neighbor_en_v1"), "draft");
}

#[test]
fn optional_norm_absence_and_oversized_timestamp_are_classified() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let without_norm = InstrumentRelease::new(
        manifest_with_norm("release_without_norm_ko_v1", RELEASE_DIGEST, None),
        40_000,
    )
    .unwrap();
    persist_ok(&mut client, &without_norm);
    let stored_norm: Option<String> = client
        .query_one(
            "SELECT norm_version_ref FROM instrument_release WHERE release_ref = $1",
            &[&"release_without_norm_ko_v1"],
        )
        .unwrap()
        .get(0);
    assert!(stored_norm.is_none());

    let overflow =
        InstrumentRelease::new(manifest("release_overflow_ko_v1", RELEASE_DIGEST), u64::MAX)
            .unwrap();
    assert!(matches!(
        persist_err(&mut client, &overflow),
        InstrumentReleasePersistenceError::InvalidTimestamp
    ));
}

#[test]
fn replay_select_failure_is_a_database_failure() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_hidden_select", RELEASE_DIGEST), 40_000).unwrap();
    persist_ok(&mut client, &draft);

    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS instrument_release_select_failure_sink;\
             CREATE OR REPLACE FUNCTION instrument_release_redirect_after_insert() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'instrument_release_select_failure_sink', false); \
                 RETURN NULL; \
             END $$; \
             CREATE TRIGGER instrument_release_redirect_after_insert \
             AFTER INSERT ON instrument_release \
             FOR EACH STATEMENT EXECUTE FUNCTION instrument_release_redirect_after_insert();",
        )
        .unwrap();

    assert!(matches!(
        persist_err(&mut client, &draft),
        InstrumentReleasePersistenceError::Database(_)
    ));
}

#[test]
fn state_advance_update_failure_is_a_database_failure() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    persist_ok(&mut client, &draft);

    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION instrument_release_reject_update() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 RAISE EXCEPTION 'instrument_release update sink'; \
             END $$; \
             CREATE TRIGGER instrument_release_reject_update \
             BEFORE UPDATE ON instrument_release \
             FOR EACH STATEMENT EXECUTE FUNCTION instrument_release_reject_update();",
        )
        .unwrap();

    let published = published_release();
    assert!(matches!(
        persist_err(&mut client, &published),
        InstrumentReleasePersistenceError::Database(_)
    ));
}

#[test]
fn unreachable_publication_state_rewind_fails_closed() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist_ok(&mut client, &published_release());
    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    assert!(matches!(
        persist_err(&mut client, &draft),
        InstrumentReleasePersistenceError::InvalidTransition
    ));
    assert_eq!(
        stored_state(&mut client, "release_big_five_ko_v1"),
        "published"
    );
}

#[test]
fn published_release_reloads_after_restart_and_stays_scoreable_for_new_sessions() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let published = published_release();
    persist_ok(&mut client, &published);

    let mut transaction = client.transaction().unwrap();
    let loaded = load_instrument_release(&mut transaction, "release_big_five_ko_v1")
        .unwrap()
        .expect("stored published release must reload after restart");
    assert!(loaded.accepts_new_sessions());
    assert_eq!(loaded.state(), PublicationState::Published);
    assert_eq!(loaded.manifest().locale(), "ko-KR");
    assert_eq!(loaded.manifest().content_digest(), RELEASE_DIGEST);
    assert_eq!(
        loaded.manifest().item_version_refs(),
        ["item_version_001", "item_version_002"]
    );
    assert_eq!(
        persist_instrument_release(&mut transaction, &loaded).unwrap(),
        InstrumentReleasePersistenceDisposition::Duplicate
    );
    transaction.commit().unwrap();
}

#[test]
fn draft_release_reloads_but_does_not_start_new_sessions() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let draft =
        InstrumentRelease::new(manifest("release_big_five_ko_v1", RELEASE_DIGEST), 40_000).unwrap();
    persist_ok(&mut client, &draft);

    let mut transaction = client.transaction().unwrap();
    let loaded = load_instrument_release(&mut transaction, "release_big_five_ko_v1")
        .unwrap()
        .expect("stored draft release must reload after restart");
    assert!(!loaded.accepts_new_sessions());
    assert_eq!(loaded.state(), PublicationState::Draft);
    transaction.commit().unwrap();
}

#[test]
fn missing_instrument_release_is_absent_after_restart() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(
        load_instrument_release(&mut transaction, "release_big_five_missing")
            .unwrap()
            .is_none()
    );
    transaction.commit().unwrap();
}

#[test]
fn instrument_release_load_rejects_blank_or_numeric_identity() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_instrument_release(&mut transaction, " "),
        Err(InstrumentReleasePersistenceError::InvalidReference)
    ));
    assert!(matches!(
        load_instrument_release(&mut transaction, "12"),
        Err(InstrumentReleasePersistenceError::InvalidReference)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn instrument_release_load_requires_read_committed_isolation() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_instrument_release(&mut transaction, "release_big_five_ko_v1"),
        Err(InstrumentReleasePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn duplicate_stored_item_versions_fail_closed_instead_of_starting_sessions() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist_ok(&mut client, &published_release());
    client
        .execute(
            "UPDATE instrument_release SET item_version_refs = ARRAY[\
                 'item_version_001', 'item_version_001'\
             ] WHERE release_ref = 'release_big_five_ko_v1'",
            &[],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_instrument_release(&mut transaction, "release_big_five_ko_v1"),
        Err(InstrumentReleasePersistenceError::InconsistentEvidence)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn startable_catalog_lists_only_published_forms_in_stable_order() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &InstrumentRelease::new(manifest("release_draft_ko_v1", RELEASE_DIGEST), 40_000).unwrap(),
    );
    persist_ok(&mut client, &review_release_named("release_review_ko_v1"));
    persist_ok(
        &mut client,
        &published_release_for("release_big_five_ko_v1", "instrument_big_five", "ko-KR"),
    );
    persist_ok(
        &mut client,
        &published_release_for("release_big_five_en_v1", "instrument_big_five", "en-US"),
    );
    persist_ok(
        &mut client,
        &published_release_for("release_alpha_en_v1", "instrument_alpha", "en-US"),
    );
    persist_ok(
        &mut client,
        &suspended_release_named("release_suspended_ko_v1"),
    );
    persist_ok(&mut client, &retired_release_named("release_retired_ko_v1"));

    let mut transaction = client.transaction().unwrap();
    let listed = list_startable_instrument_releases(&mut transaction).unwrap();
    let identities: Vec<(&str, &str, &str)> = listed
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
            ("instrument_alpha", "en-US", "release_alpha_en_v1"),
            ("instrument_big_five", "en-US", "release_big_five_en_v1"),
            ("instrument_big_five", "ko-KR", "release_big_five_ko_v1"),
        ]
    );
    for release in &listed {
        assert!(release.accepts_new_sessions());
        assert_eq!(
            persist_instrument_release(&mut transaction, release).unwrap(),
            InstrumentReleasePersistenceDisposition::Duplicate
        );
    }
    transaction.commit().unwrap();
}

#[test]
fn startable_catalog_is_empty_when_no_published_form_is_stored() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist_ok(
        &mut client,
        &InstrumentRelease::new(manifest("release_draft_ko_v1", RELEASE_DIGEST), 40_000).unwrap(),
    );

    let mut transaction = client.transaction().unwrap();
    assert!(list_startable_instrument_releases(&mut transaction)
        .unwrap()
        .is_empty());
    transaction.commit().unwrap();
}

#[test]
fn startable_catalog_fails_closed_on_corrupt_published_row() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    persist_ok(&mut client, &published_release());
    persist_ok(
        &mut client,
        &published_release_for("release_alpha_en_v1", "instrument_alpha", "en-US"),
    );
    client
        .execute(
            "UPDATE instrument_release SET item_version_refs = ARRAY[\
                 'item_version_001', 'item_version_001'\
             ] WHERE release_ref = 'release_big_five_ko_v1'",
            &[],
        )
        .unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        list_startable_instrument_releases(&mut transaction),
        Err(InstrumentReleasePersistenceError::InconsistentEvidence)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn startable_catalog_requires_read_committed_isolation() {
    let _guard = instrument_release_test_guard();
    let mut client = test_client();
    reset_instrument_release_tables(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        list_startable_instrument_releases(&mut transaction),
        Err(InstrumentReleasePersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}
