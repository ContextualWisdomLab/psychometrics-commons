//! Traceability regression coverage for active work versus protected-main truth.

use std::fs;
use std::path::PathBuf;

#[test]
fn active_work_is_not_protected_main_truth() {
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
        .find(|line| line.contains("**Active PR**"))
        .expect("traceability must name active PR work in its active-work section");

    assert!(
        pr_entry.contains("not protected-main truth") && !pr_entry.contains("**Implemented**"),
        "active implementation work must remain explicitly segregated from protected-main truth"
    );
}

#[test]
fn adr_0020_implementation_status_names_the_traceability_identity_landing() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let traceability = fs::read_to_string(root.join("docs/TRACEABILITY.md"))
        .expect("traceability document must be readable");
    let adr = fs::read_to_string(
        root.join("docs/adr/0020-append-only-participant-identity-link-history.md"),
    )
    .expect("ADR-0020 must be readable");
    let active_work = traceability
        .split("### Active implementation work that is not protected-main truth")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("traceability must define the active implementation-work section");
    let landing = active_work
        .split_whitespace()
        .find(|token| {
            token.starts_with('#')
                && token
                    .trim_start_matches('#')
                    .chars()
                    .all(|character| character.is_ascii_digit())
        })
        .expect("active identity-link work must name a landing pull request");
    let implementation_status = adr
        .split("### Implementation status")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("ADR-0020 must declare implementation status");

    assert!(
        implementation_status.contains(&format!("PR {landing}"))
            && implementation_status.contains(&format!("Prefer {landing}")),
        "ADR-0020 Implementation status must name TRACEABILITY landing {landing} so concurrent writers do not treat a superseded identity-link PR as the persist vehicle"
    );
}
