//! Direct-SQL parity between `PostgreSQL` audit identities and the current Rust opaque-reference boundary.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
static AUDIT_PARITY_TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    AUDIT_PARITY_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_reference_parity_test CASCADE;\
             CREATE SCHEMA audit_reference_parity_test;\
             SET search_path TO audit_reference_parity_test;",
        )
        .unwrap();
    client
}

fn assert_direct_write_rejected(client: &mut Client, candidate: &str) {
    let result = client.execute(
        "INSERT INTO audit_evidence_record (\
            audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
            outcome_code, evidence_digest, occurred_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        &[
            &candidate,
            &"tenant_research_alpha",
            &"actor_publisher_alpha",
            &"instrument_publication",
            &"publish_instrument_release",
            &"instrument_release_big_five_ko_v1",
            &"succeeded",
            &DIGEST,
            &1_785_000_000_000_i64,
        ],
    );
    assert!(
        result.is_err(),
        "direct SQL must reject a Rust-invalid opaque reference: {candidate:?}"
    );
}

#[test]
fn database_rejects_control_default_ignorable_and_unicode_numeric_aliases() {
    let _guard = test_guard();
    let mut client = client();
    apply_audit_evidence_migration(&mut client).unwrap();

    for invalid in [
        "audit_event_\u{0001}_control",
        "audit_event_\u{200b}_zero_width",
        "audit_event_\u{200d}_joiner",
        "audit_event_\u{2060}_word_joiner",
        "audit_event_\u{fe0f}_variation",
        "audit_event_\u{e0001}_tag",
        "²",
        "Ⅻ",
    ] {
        assert_direct_write_rejected(&mut client, invalid);
    }

    let accepted = client
        .execute(
            "INSERT INTO audit_evidence_record (\
                audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
                outcome_code, evidence_digest, occurred_at_unix_ms\
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[
                &"audit_event_가나다_東京_éclair",
                &"tenant_research_alpha",
                &"actor_publisher_alpha",
                &"instrument_publication",
                &"publish_instrument_release",
                &"instrument_release_big_five_ko_v1",
                &"succeeded",
                &DIGEST,
                &1_785_000_000_000_i64,
            ],
        )
        .unwrap();
    assert_eq!(
        accepted, 1,
        "visible multilingual identity material must remain valid"
    );
}
