//! Traceability regression coverage for active work versus protected-main truth.

use std::fs;
use std::path::PathBuf;

#[test]
fn active_persistence_pr_is_not_protected_main_truth() {
    let traceability_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/TRACEABILITY.md");
    let traceability = fs::read_to_string(traceability_path)
        .expect("traceability document must be readable");
    let pr_entry = traceability
        .lines()
        .find(|line| line.starts_with("PR #24 "))
        .expect("traceability must name PR #24 in its active-work section");

    assert!(
        pr_entry.contains("**Active PR**")
            && pr_entry.contains("integrated into protected main")
            && !pr_entry.contains("**Implemented**"),
        "active persistence work must remain explicitly segregated from protected-main truth"
    );
}
