//! Real `PostgreSQL` contract: already-shown items survive process restart.
//!
//! A buyer mid-assessment must see the same delivered item set after the
//! runtime reloads durable evidence. Reload must not invent deliveries, leak
//! another tenant's session, or accept a stronger isolation level that can hide
//! a concurrent append.

use postgres::{Client, IsolationLevel, NoTls, Transaction};
use psychometrics_commons_runtime::instrument::InstrumentReleaseManifest;
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::postgres_item_delivery::{
    apply_item_delivery_migration, load_item_delivery_ledger, persist_item_delivery_ledger,
    ItemDeliveryPersistenceDisposition, ItemDeliveryPersistenceError,
};
use psychometrics_commons_runtime::session::SessionState;
use std::sync::{Mutex, MutexGuard};

const TENANT_REF: &str = "tenant_item_delivery_reload";
const OTHER_TENANT_REF: &str = "tenant_item_delivery_reload_other";
const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

static ITEM_DELIVERY_RELOAD_LOCK: Mutex<()> = Mutex::new(());

fn item_delivery_reload_guard() -> MutexGuard<'static, ()> {
    ITEM_DELIVERY_RELOAD_LOCK
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
            "CREATE SCHEMA IF NOT EXISTS item_delivery_reload_test;\
             SET search_path TO item_delivery_reload_test;",
        )
        .unwrap();
    client
}

fn reset_item_delivery_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS item_delivery_reload_test.item_delivery_event;\
             DROP TABLE IF EXISTS item_delivery_reload_test.item_delivery_ledger;",
        )
        .unwrap();
}

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
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
        RELEASE_DIGEST,
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
    deliveries: &[(&str, &str, &str, Option<&str>)],
) -> ItemDeliveryLedger {
    let mut ledger = ItemDeliveryLedger::from_manifest(session_ref, &manifest()).unwrap();
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

fn persist_ok(
    client: &mut Client,
    ledger: &ItemDeliveryLedger,
) -> ItemDeliveryPersistenceDisposition {
    let mut transaction = client.transaction().unwrap();
    let disposition = persist_item_delivery_ledger(&mut transaction, TENANT_REF, ledger).unwrap();
    transaction.commit().unwrap();
    disposition
}

fn load_ok(client: &mut Client, session_ref: &str) -> Option<ItemDeliveryLedger> {
    let mut transaction = client.transaction().unwrap();
    let loaded = load_item_delivery_ledger(&mut transaction, TENANT_REF, session_ref).unwrap();
    transaction.commit().unwrap();
    loaded
}

fn load_err(
    transaction: &mut Transaction<'_>,
    tenant_ref: &str,
    session_ref: &str,
) -> ItemDeliveryPersistenceError {
    load_item_delivery_ledger(transaction, tenant_ref, session_ref).unwrap_err()
}

#[test]
fn unknown_session_reload_is_absent_not_an_empty_delivery_list() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    assert!(
        load_ok(&mut client, "session_item_delivery_reload_unknown").is_none(),
        "a session that never persisted deliveries must not appear started after restart"
    );
}

#[test]
fn empty_persisted_ledger_reloads_without_inventing_items() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let empty =
        ItemDeliveryLedger::from_manifest("session_item_delivery_reload_empty", &manifest())
            .unwrap();
    assert_eq!(
        persist_ok(&mut client, &empty),
        ItemDeliveryPersistenceDisposition::Inserted
    );
    let loaded = load_ok(&mut client, "session_item_delivery_reload_empty")
        .expect("an empty persisted ledger must reload");
    assert_eq!(loaded, empty);
    assert!(loaded.is_empty());
}

#[test]
fn delivered_items_reload_in_server_sequence_after_restart() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let live = delivered_ledger(
        "session_item_delivery_reload_order",
        &[
            (
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                Some("selection_adaptive_v1"),
            ),
            (
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
        ],
    );
    assert_eq!(
        persist_ok(&mut client, &live),
        ItemDeliveryPersistenceDisposition::Inserted
    );
    let loaded = load_ok(&mut client, "session_item_delivery_reload_order")
        .expect("a delivered ledger must reload after restart");
    assert_eq!(loaded, live);
    assert_eq!(loaded.events()[0].item_version_ref(), "item_version_002");
    assert_eq!(loaded.events()[1].item_version_ref(), "item_version_001");
    assert_eq!(loaded.events()[0].sequence(), 1);
    assert_eq!(loaded.events()[1].sequence(), 2);
}

#[test]
fn other_tenant_cannot_reload_a_persisted_session_as_absent() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let live = delivered_ledger(
        "session_item_delivery_reload_tenant",
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    persist_ok(&mut client, &live);
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_err(
                &mut transaction,
                OTHER_TENANT_REF,
                "session_item_delivery_reload_tenant",
            ),
            ItemDeliveryPersistenceError::ConflictingReplay
        ),
        "another tenant must not treat a persisted session as a new empty assessment"
    );
    transaction.rollback().unwrap();
}

#[test]
fn sequence_gap_reload_fails_closed_instead_of_skipping_an_item() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let live = delivered_ledger(
        "session_item_delivery_reload_gap",
        &[
            (
                "delivery_event_001",
                "item_version_001",
                "presentation_standard_v1",
                None,
            ),
            (
                "delivery_event_002",
                "item_version_002",
                "presentation_standard_v1",
                None,
            ),
        ],
    );
    persist_ok(&mut client, &live);
    client
        .execute(
            "UPDATE item_delivery_event SET delivery_sequence = 3 \
             WHERE session_ref = $1 AND delivery_event_ref = $2",
            &[&"session_item_delivery_reload_gap", &"delivery_event_002"],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_err(
            &mut transaction,
            TENANT_REF,
            "session_item_delivery_reload_gap",
        ),
        ItemDeliveryPersistenceError::CorruptHistory
    ));
    transaction.rollback().unwrap();
}

#[test]
fn stored_item_outside_allowed_set_fails_closed_on_reload() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let live = delivered_ledger(
        "session_item_delivery_reload_foreign_item",
        &[(
            "delivery_event_001",
            "item_version_001",
            "presentation_standard_v1",
            None,
        )],
    );
    persist_ok(&mut client, &live);
    client
        .execute(
            "UPDATE item_delivery_event SET item_version_ref = $1 \
             WHERE session_ref = $2 AND delivery_event_ref = $3",
            &[
                &"item_version_003",
                &"session_item_delivery_reload_foreign_item",
                &"delivery_event_001",
            ],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(
        matches!(
            load_err(
                &mut transaction,
                TENANT_REF,
                "session_item_delivery_reload_foreign_item",
            ),
            ItemDeliveryPersistenceError::CorruptHistory
        ),
        "a stored item outside the allowed set must not reload as a new presentation"
    );
    transaction.rollback().unwrap();
}

#[test]
fn padded_tenant_alias_fails_closed_on_reload() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_err(
            &mut transaction,
            " tenant_item_delivery_reload",
            "session_item_delivery_reload_tenant_pad",
        ),
        ItemDeliveryPersistenceError::InvalidReference
    ));
    transaction.rollback().unwrap();
}

#[test]
fn padded_session_alias_fails_closed_on_reload() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_err(
            &mut transaction,
            TENANT_REF,
            " session_item_delivery_reload_pad",
        ),
        ItemDeliveryPersistenceError::InvalidReference
    ));
    transaction.rollback().unwrap();
}

#[test]
fn stronger_isolation_fails_closed_on_reload() {
    let _guard = item_delivery_reload_guard();
    let mut client = test_client();
    reset_item_delivery_tables(&mut client);
    apply_item_delivery_migration(&mut client).unwrap();

    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_err(
            &mut transaction,
            TENANT_REF,
            "session_item_delivery_reload_isolation",
        ),
        ItemDeliveryPersistenceError::UnsupportedIsolationLevel
    ));
    transaction.rollback().unwrap();
}
