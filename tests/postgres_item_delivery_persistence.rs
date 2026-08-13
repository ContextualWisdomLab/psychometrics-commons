//! Real `PostgreSQL` contract for durable item-delivery ledger evidence.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static ITEM_DELIVERY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn item_delivery_test_guard() -> MutexGuard<'static, ()> {
    ITEM_DELIVERY_TEST_LOCK
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

fn manifest(release_ref: &str, digest: &str) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
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
            persist_item_delivery_ledger(&mut transaction, &ledger).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, &ledger).unwrap(),
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
        persist_item_delivery_ledger(&mut transaction, &rebound),
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
            persist_item_delivery_ledger(&mut transaction, &first).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, &first).unwrap(),
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
        persist_item_delivery_ledger(&mut transaction, &conflicting_presentation),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn later_accepted_events_append_and_independent_first_sequences_conflict() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let first = delivered_ledger(
        "session_item_delivery_gamma",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            Some("selection_fixed_order_v1"),
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_item_delivery_ledger(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }

    let both = delivered_ledger(
        "session_item_delivery_gamma",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[
            (
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
            (
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("selection_fixed_order_v1"),
            ),
        ],
    );
    {
        let mut transaction = client.transaction().unwrap();
        assert_eq!(
            persist_item_delivery_ledger(&mut transaction, &both).unwrap(),
            ItemDeliveryPersistenceDisposition::Inserted
        );
        transaction.commit().unwrap();
    }

    let sequenced = delivered_ledger(
        "session_item_delivery_gamma_sequence",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            Some("selection_fixed_order_v1"),
        )],
    );
    {
        let mut transaction = client.transaction().unwrap();
        persist_item_delivery_ledger(&mut transaction, &sequenced).unwrap();
        transaction.commit().unwrap();
    }

    let independent_first_sequence = delivered_ledger(
        "session_item_delivery_gamma_sequence",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_orphan_first",
            "item_version_002",
            "presentation_standard_v1",
            Some("selection_fixed_order_v1"),
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, &independent_first_sequence),
        Err(ItemDeliveryPersistenceError::SequenceConflict)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn duplicate_item_under_a_new_delivery_identity_fails_closed() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let first = delivered_ledger(
        "session_item_delivery_delta",
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
        persist_item_delivery_ledger(&mut transaction, &first).unwrap();
        transaction.commit().unwrap();
    }

    let reused_item = delivered_ledger(
        "session_item_delivery_delta",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_002",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, &reused_item),
        Err(ItemDeliveryPersistenceError::DuplicateItemDelivery)
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
        persist_item_delivery_ledger(&mut transaction, &ledger),
        Err(ItemDeliveryPersistenceError::UnsupportedIsolationLevel)
    ));
    transaction.rollback().unwrap();
}

#[test]
fn missing_event_relation_is_classified_as_a_database_failure() {
    let _guard = item_delivery_test_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();
    client
        .batch_execute("DROP TABLE item_delivery_persistence_test.item_delivery_event;")
        .unwrap();

    let ledger = delivered_ledger(
        "session_item_delivery_missing_event",
        "release_big_five_ko_v1",
        RELEASE_DIGEST,
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut transaction, &ledger),
        Err(ItemDeliveryPersistenceError::Database(_))
    ));
    transaction.rollback().unwrap();
}
