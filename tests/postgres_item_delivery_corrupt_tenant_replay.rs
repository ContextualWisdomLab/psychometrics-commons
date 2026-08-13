//! Corruption regression for fail-closed item-delivery replay classification.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, persist_item_delivery_ledger, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;

const DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS item_delivery_corrupt_tenant_test CASCADE;\
             CREATE SCHEMA item_delivery_corrupt_tenant_test;\
             SET search_path TO item_delivery_corrupt_tenant_test;",
        )
        .unwrap();
    apply_item_delivery_migration(&mut client).unwrap();
    client
}

fn delivered_ledger() -> ItemDeliveryLedger {
    let manifest = InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_ko_v1",
        "construct_big_five",
        &["item_version_001"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_big_five_v1",
        "calibration_big_five_v1",
        Some("norm_big_five_ko_v1"),
        "narrative_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_big_five_v1",
        DIGEST,
    )
    .unwrap();
    let mut ledger =
        ItemDeliveryLedger::from_manifest("session_corrupt_tenant_replay", &manifest).unwrap();
    ledger
        .deliver(
            SessionState::Active,
            ItemDeliveryRequest {
                delivery_ref: "delivery_corrupt_tenant_replay",
                item_version_ref: "item_version_001",
                presentation_context_ref: "presentation_standard_v1",
                selection_evidence_ref: None,
            },
        )
        .unwrap();
    ledger
}

#[test]
fn replay_fails_closed_when_stored_event_tenant_no_longer_matches_ledger() {
    let mut client = test_client();
    let ledger = delivered_ledger();

    {
        let mut transaction = client.transaction().unwrap();
        persist_item_delivery_ledger(&mut transaction, "tenant_original", &ledger).unwrap();
        transaction.commit().unwrap();
    }

    client
        .batch_execute(
            "ALTER TABLE item_delivery_event \
                 DROP CONSTRAINT item_delivery_event_session_tenant_fk;\
             UPDATE item_delivery_event \
                 SET tenant_ref = 'tenant_corrupt' \
                 WHERE session_ref = 'session_corrupt_tenant_replay' \
                   AND delivery_event_ref = 'delivery_corrupt_tenant_replay';",
        )
        .unwrap();

    let mut replay = client.transaction().unwrap();
    assert!(matches!(
        persist_item_delivery_ledger(&mut replay, "tenant_original", &ledger),
        Err(ItemDeliveryPersistenceError::ConflictingReplay)
    ));
    replay.rollback().unwrap();
}
