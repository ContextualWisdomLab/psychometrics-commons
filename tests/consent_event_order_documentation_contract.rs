//! Documentation contract for durable consent-ledger event ordering.
//!
//! Migration 0021 makes `consent_event.event_sequence` the persisted ordering
//! authority on this active PR. The logical ERD must describe that field and its
//! per-participant uniqueness without promoting the active PR to protected-main
//! truth.

use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn consent_event_order_migration_is_mapped_in_the_logical_erd() {
    let root = repository_root();
    let migration = fs::read_to_string(root.join("migrations/0021_consent_event_order.sql"))
        .expect("consent event ordering migration must be readable");
    let erd = fs::read_to_string(root.join("docs/architecture/ERD.md"))
        .expect("logical ERD must be readable");

    assert!(
        migration.contains("ADD COLUMN event_sequence BIGINT")
            && migration.contains("CHECK (event_sequence IS NULL OR event_sequence > 0)")
            && migration.contains("ON consent_event (participant_ref, event_sequence)")
            && migration.contains("WHERE event_sequence IS NOT NULL"),
        "migration 0021 must retain the positive, per-participant consent sequence contract"
    );

    let consent_event_block = erd
        .split("    consent_event {")
        .nth(1)
        .and_then(|section| section.split("    }").next())
        .expect("logical ERD must define consent_event");
    assert!(
        consent_event_block.contains("int event_sequence"),
        "logical ERD consent_event must expose event_sequence"
    );
    assert!(
        erd.contains("unique `(participant_ref, event_sequence)`"),
        "logical ERD must describe per-participant consent event ordering uniqueness"
    );
    assert!(
        erd.contains("Active PR #292") && erd.contains("not protected-main truth"),
        "ERD must keep migration 0021 maturity explicit until the PR is integrated"
    );
}
