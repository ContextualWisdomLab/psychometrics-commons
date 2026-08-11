//! Traceability regression coverage for active work versus protected-main truth.

use std::fs;
use std::path::PathBuf;

#[test]
fn active_persistence_pr_is_not_protected_main_truth() {
    let traceability_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/TRACEABILITY.md");
    let traceability =
        fs::read_to_string(traceability_path).expect("traceability document must be readable");
    let active_work = traceability
        .split("### Active implementation work that is not protected-main truth")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("traceability must define the active implementation-work section");
    let pr_entry = active_work
        .lines()
        .find(|line| line.contains("PR #24"))
        .expect("traceability must name PR #24 in its active-work section");

    assert!(
        pr_entry.contains("**Active PR**")
            && pr_entry.contains("**Active PR** until")
            && pr_entry.contains("integrated into protected main")
            && !pr_entry.contains("**Implemented**"),
        "active persistence work must remain explicitly segregated from protected-main truth"
    );
}
