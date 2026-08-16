//! Published-release load used for session start must lock the row.
//!
//! A start that only reads `publication_state` can insert after a concurrent
//! Suspend or Retire commits. The load used by
//! `start_created_assessment_session_from_stored_release` must take
//! `SELECT … FOR UPDATE` so the operator who already classified the release as
//! startable cannot lose that row before the session insert commits.

use std::fs;
use std::path::PathBuf;

fn instrument_release_adapter_source() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/postgres_instrument_release.rs"),
    )
    .expect("instrument-release adapter source must be readable")
}

fn load_published_instrument_release_body(source: &str) -> &str {
    let after_signature = source
        .split("pub fn load_published_instrument_release(")
        .nth(1)
        .expect("load_published_instrument_release must exist");
    let body_start = after_signature
        .find('{')
        .expect("load_published_instrument_release must have a body");
    after_signature[body_start..]
        .split("\n}\n")
        .next()
        .expect("load_published_instrument_release body must close")
}

#[test]
fn published_release_load_locks_the_row_for_the_caller_transaction() {
    let source = instrument_release_adapter_source();
    let load = load_published_instrument_release_body(&source);
    assert!(
        load.contains("FOR UPDATE"),
        "published-release load must lock the row so a concurrent Suspend or Retire cannot hide from session start"
    );
}
