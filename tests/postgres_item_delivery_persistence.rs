//! Real `PostgreSQL` contract for durable tenant-bound item-delivery evidence.

use postgres::{Client, IsolationLevel, NoTls, Transaction};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;

const TENANT_REF: &str = "tenant_item_delivery_alpha";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY: i64 = 0x4954_454D_4445_4C56;

fn item_delivery_test_guard() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY],
        )
        .expect("PostgreSQL fixture advisory lock should be acquired");
    guard
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS item_delivery_persistence_test;\
             SET search_path TO item_delivery_persistence_test;",
        )
        .unwrap();
    client
}

fn reset_item_delivery_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS item_delivery_persistence_test.item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_persistence_test.item_delivery_ledger;",
        )
        .unwrap();
}

fn manifest_parts(
    release_ref: &str,
    item_version_refs: &[&str],
    locale: &str,
    digest: &str,
) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        item_version_refs,
        locale,
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_big_five_ko_v1"),
        "narrative_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        digest,
    )
    .unwrap()
}

fn manifest(release_ref: &str, digest: &str) -> InstrumentReleaseManifest {
    manifest_parts(
        release_ref,
        &["item_version_001", "item_version_002"],
        "ko-KR",
        digest,
    )
}

fn request<'a>(
    delivery_ref: &'a str,
    item_version_ref: &'a str,
    presentation_context_ref: &'a str,
    selection_evidence_ref: Option<&'a str>,
) -> ItemDeliveryRequest<'a> {
    ItemDeliveryRequest {
        delivery_ref,
        item_version_ref,
        presentation_context_ref,
        selection_evidence_ref,
    }
}

fn delivered_ledger(
    session_ref: &str,
    release_ref: &str,
    digest: &str,
    deliveries: &[(&str, &str, &str, Option<&str>)],
) -> ItemDeliveryLedger {
    let mut ledger =
        ItemDeliveryLedger::from_manifest(session_ref, &manifest(release_ref, digest)).unwrap();
    for (delivery_ref, item_version_ref, presentation_context_ref, selection_evidence_ref) in
        deliveries
    {
        ledger
            .deliver(
                SessionState::Active,
                request(
                    delivery_ref,
                    item_version_ref,
                    presentation_context_ref,
                    *selection_evidence_ref,
                ),
            )
            .unwrap();
    }
    ledger
}

fn persist(
    transaction: &mut Transaction<'_>,
    ledger: &ItemDeliveryLedger,
) -> Result<ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError> {
    persist_item_delivery_ledger(transaction, TENANT_REF, ledger)
}

#[test]
fn fixture_lock_is_visible_across_database_sessions() {
    let _guard = item_delivery_test_guard();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY],
        )
        .unwrap()
        .get(0);

    if acquired {
        contender
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&ITEM_DELIVERY_PERSISTENCE_DATABASE_LOCK_KEY],
            )
            .unwrap();
    }

    assert!(
        !acquired,
        "fixture serialization must be enforced by PostgreSQL, not only by a process-local mutex"
    );
}

#[test]
fn empty_ledger_persist_is_exactly_idempotent_and_release_rebinding_fails_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let ledger = ItemDeliveryLedger::from_manifest(
        "session_item_delivery_alpha",
        &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
    )
    .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist(&mut transaction, &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist(&mut transaction, &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let rebound = ItemDeliveryLedger::from_manifest(
        "session_item_delivery_alpha",
        &manifest("release_big_five_en_v1", OTHER_DIGEST),
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &rebound),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn tenant_rebinding_fails_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = ItemDeliveryLedger::from_manifest(
        "session_item_delivery_tenant_rebind",
        &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
    )
    .unwrap();

    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, &ledger).unwrap();
        transaction.commit().unwrap();
    }
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, "tenant_item_delivery_beta", &ledger),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn accepted_delivery_events_are_idempotent_and_conflicting_evidence_fails_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let first = delivered_ledger(
        "session_item_delivery_beta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist(&mut transaction, &first).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist(&mut transaction, &first).unwrap(),
            ItemDeliveryPersistenceDisposition::Duplicate
        );
        transaction.commit().unwrap();
    }

    let conflicting_presentation = delivered_ledger(
        "session_item_delivery_beta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_large_type_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &conflicting_presentation),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn later_events_append_and_independent_first_sequences_conflict() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let first = delivered_ledger(
        "session_item_delivery_gamma",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_gamma_001",
            "item_version_001",
            "presentation_standard_v1",
            Some("selection_fixed_order_v1"),
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }

    let both = delivered_ledger(
        "session_item_delivery_gamma",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[
            (
                "delivery_event_gamma_001",
                "item_version_001",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
            (
                "delivery_event_gamma_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
        ],
    );
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist(&mut transaction, &both).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let sequenced = delivered_ledger(
        "session_item_delivery_gamma_sequence",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_sequence_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, &sequenced).unwrap();
        transaction.commit().unwrap();
    }
    let independent_first = delivered_ledger(
        "session_item_delivery_gamma_sequence",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_sequence_002",
            "item_version_002",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &independent_first),
        Err(ItemDeliveryPersistenceError::SequenceConflict)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn duplicate_item_classification_does_not_depend_on_unique_constraint_order() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    client
        .execute(
            "INSERT INTO item_delivery_ledger (tenant_ref, session_ref, instrument_release_ref, \
             release_content_digest, locale, allowed_item_version_refs) \
             VALUES ($1, 'session_item_delivery_delta', 'release_big_five_ko_v1', $2, 'ko-KR', \
             ARRAY['item_version_001', 'item_version_002'])",
            &[&TENANT_REF, &RELEASE_DIGEST],
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO item_delivery_event (tenant_ref, session_ref, delivery_event_ref, \
             item_version_ref, presentation_context_ref, delivery_sequence) \
             VALUES ($1, 'session_item_delivery_delta', 'delivery_event_seed', \
             'item_version_001', 'presentation_standard_v1', 2)",
            &[&TENANT_REF],
        )
        .unwrap();

    let reused_item = delivered_ledger(
        "session_item_delivery_delta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_reused_item",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &reused_item),
        Err(ItemDeliveryPersistenceError::DuplicateItemDelivery)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn globally_reused_delivery_identity_fails_closed_across_tenants() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let first = delivered_ledger(
        "session_global_delivery_alpha",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_global_identity",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_item_delivery_ledger(&mut transaction, "tenant_global_alpha", &first).unwrap();
        transaction.commit().unwrap();
    }

    let second = delivered_ledger(
        "session_global_delivery_beta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_global_identity",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, "tenant_global_beta", &second),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn item_delivery_persistence_requires_read_committed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    let ledger = ItemDeliveryLedger::from_manifest(
        "session_item_delivery_serializable",
        &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
    )
    .unwrap();
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist(&mut transaction, &ledger),
        Err(ItemDeliveryPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_relations_are_database_failures() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    let empty = ItemDeliveryLedger::from_manifest(
        "session_item_delivery_missing_ledger",
        &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
    )
    .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist(&mut transaction, &empty),
            Err(ItemDeliveryPersistenceError::Database(_))
        ));
        transaction.rollback().unwrap();
    }

    apply_item_delivery_migration(&mut client).unwrap();
    client
        .batch_execute("DROP TABLE item_delivery_event;")
        .unwrap();
    let with_event = delivered_ledger(
        "session_item_delivery_missing_event",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_missing_relation",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &with_event),
        Err(ItemDeliveryPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

#[test]
fn unknown_unique_constraint_is_a_database_failure() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    client
        .batch_execute(
            "ALTER TABLE item_delivery_event ADD CONSTRAINT \
             item_delivery_event_presentation_test_unique UNIQUE (presentation_context_ref);",
        )
        .unwrap();

    let first = delivered_ledger(
        "session_unknown_unique_alpha",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_unknown_unique_alpha",
            "item_version_001",
            "presentation_unknown_unique",
            None,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }
    let second = delivered_ledger(
        "session_unknown_unique_beta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_unknown_unique_beta",
            "item_version_001",
            "presentation_unknown_unique",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &second),
        Err(ItemDeliveryPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}

fn persist_then_conflict(
    client: &mut Client,
    original: &ItemDeliveryLedger,
    conflicting: &ItemDeliveryLedger,
) {
    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, original).unwrap();
        transaction.commit().unwrap();
    }
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, conflicting),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn digest_locale_and_allowed_item_rebinding_fail_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    persist_then_conflict(
        &mut client,
        &ItemDeliveryLedger::from_manifest(
            "session_digest_conflict",
            &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
        )
        .unwrap(),
        &ItemDeliveryLedger::from_manifest(
            "session_digest_conflict",
            &manifest("release_big_five_ko_v1", OTHER_DIGEST),
        )
        .unwrap(),
    );
    persist_then_conflict(
        &mut client,
        &ItemDeliveryLedger::from_manifest(
            "session_locale_conflict",
            &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
        )
        .unwrap(),
        &ItemDeliveryLedger::from_manifest(
            "session_locale_conflict",
            &manifest_parts(
                "release_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
                "en-US",
                RELEASE_DIGEST,
            ),
        )
        .unwrap(),
    );
    persist_then_conflict(
        &mut client,
        &ItemDeliveryLedger::from_manifest(
            "session_allowed_conflict",
            &manifest("release_big_five_ko_v1", RELEASE_DIGEST),
        )
        .unwrap(),
        &ItemDeliveryLedger::from_manifest(
            "session_allowed_conflict",
            &manifest_parts(
                "release_big_five_ko_v1",
                &["item_version_001", "item_version_003"],
                "ko-KR",
                RELEASE_DIGEST,
            ),
        )
        .unwrap(),
    );
}

#[test]
fn selection_item_and_sequence_mismatches_fail_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    persist_then_conflict(
        &mut client,
        &delivered_ledger(
            "session_selection_conflict",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            &[(
                "delivery_event_selection",
                "item_version_001",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            )],
        ),
        &delivered_ledger(
            "session_selection_conflict",
            "release_big_five_ko_v1",
            RELEASE_DIGEST,
            &[(
                "delivery_event_selection",
                "item_version_001",
                "presentation_standard_v1",
                Some("selection_adaptive_v1"),
            )],
        ),
    );

    let ledger = delivered_ledger(
        "session_stored_field_mismatch",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_stored_field",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist(&mut transaction, &ledger).unwrap();
        transaction.commit().unwrap();
    }
    client
        .batch_execute(
            "UPDATE item_delivery_event SET item_version_ref = 'item_version_002' \
             WHERE session_ref = 'session_stored_field_mismatch';",
        )
        .unwrap();
    {
        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            persist(&mut transaction, &ledger),
            Err(ItemDeliveryPersistenceError::ConflictingReplay)
        ));
        transaction.rollback().unwrap();
    }
    client
        .batch_execute(
            "UPDATE item_delivery_event SET item_version_ref = 'item_version_001', \
             delivery_sequence = 99 WHERE session_ref = 'session_stored_field_mismatch';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist(&mut transaction, &ledger),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}
