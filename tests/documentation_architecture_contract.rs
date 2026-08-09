//! Repository architecture-documentation fitness checks.
//!
//! These tests keep the required product/architecture/governance viewpoints
//! discoverable and prevent an accepted ADR from being added without the
//! authoritative ADR index knowing about it. They intentionally validate
//! repository structure and traceability markers, not Mermaid layout or semantic
//! correctness; human architecture review remains required for those.

use std::fs;
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_required(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "required documentation {} is unreadable: {error}",
            path.display()
        )
    })
}

#[test]
fn required_architecture_and_governance_viewpoints_exist() {
    let root = repository_root();
    let required_paths = [
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "ARCHITECTURE.md",
        "CHANGELOG.md",
        "docs/PRD.md",
        "docs/TRD.md",
        "docs/MEASUREMENT_GOVERNANCE.md",
        "docs/AI_GOVERNANCE.md",
        "docs/RESEARCH_GOVERNANCE.md",
        "docs/QUALITY_ATTRIBUTES.md",
        "docs/COMPLIANCE_READINESS.md",
        "docs/RISK_REGISTER.md",
        "docs/GLOSSARY.md",
        "docs/DOCUMENTATION_ASSESSMENT.md",
        "docs/TRACEABILITY.md",
        "docs/ROADMAP.md",
        "docs/architecture/README.md",
        "docs/architecture/C4.md",
        "docs/architecture/UML.md",
        "docs/architecture/ERD.md",
        "docs/architecture/SECURITY_AND_DATA.md",
        "docs/architecture/DEPLOYMENT_AND_OPERATIONS.md",
        "docs/adr/README.md",
        "docs/adr/0000-template.md",
    ];

    for relative_path in required_paths {
        let path = root.join(relative_path);
        assert!(
            path.is_file(),
            "required architecture/governance artifact is missing: {relative_path}"
        );
        assert!(
            !read_required(&path).trim().is_empty(),
            "required architecture/governance artifact is empty: {relative_path}"
        );
    }
}

#[test]
fn adr_index_lists_every_numbered_decision_file() {
    let root = repository_root();
    let adr_directory = root.join("docs/adr");
    let index = read_required(&adr_directory.join("README.md"));

    let entries = fs::read_dir(&adr_directory).expect("docs/adr must be readable");
    for entry in entries {
        let entry = entry.expect("ADR directory entry must be readable");
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let is_numbered_adr = file_name.ends_with(".md")
            && file_name.len() >= 9
            && file_name.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && !file_name.starts_with("0000-");
        if !is_numbered_adr {
            continue;
        }

        assert!(
            index.contains(file_name.as_ref()),
            "ADR index does not reference decision file {file_name}"
        );
    }
}

#[test]
fn repository_entry_points_expose_traceability_and_architecture_views() {
    let root = repository_root();
    let readme = read_required(&root.join("README.md"));
    let architecture = read_required(&root.join("ARCHITECTURE.md"));

    for required_link in ["docs/TRACEABILITY.md", "docs/DOCUMENTATION_ASSESSMENT.md"] {
        assert!(
            readme.contains(required_link),
            "README must expose {required_link}"
        );
        assert!(
            architecture.contains(required_link),
            "ARCHITECTURE.md must expose {required_link}"
        );
    }

    assert!(
        readme.contains("docs/architecture/README.md"),
        "README must expose the architecture view index"
    );
    for view_link in [
        "docs/architecture/C4.md",
        "docs/architecture/UML.md",
        "docs/architecture/ERD.md",
        "docs/architecture/SECURITY_AND_DATA.md",
        "docs/architecture/DEPLOYMENT_AND_OPERATIONS.md",
    ] {
        assert!(
            architecture.contains(view_link),
            "ARCHITECTURE.md must expose architecture view {view_link}"
        );
    }

    for governance_link in [
        "docs/MEASUREMENT_GOVERNANCE.md",
        "docs/AI_GOVERNANCE.md",
        "docs/RESEARCH_GOVERNANCE.md",
        "docs/QUALITY_ATTRIBUTES.md",
        "docs/COMPLIANCE_READINESS.md",
        "docs/RISK_REGISTER.md",
        "docs/GLOSSARY.md",
    ] {
        assert!(
            readme.contains(governance_link),
            "README must expose {governance_link}"
        );
    }
}

#[test]
fn traceability_distinguishes_current_implementation_from_targets() {
    let traceability = read_required(&repository_root().join("docs/TRACEABILITY.md"));

    for status in [
        "Implemented",
        "Partially implemented",
        "Target",
        "External dependency",
    ] {
        assert!(
            traceability.contains(status),
            "traceability document must define status {status}"
        );
    }

    assert!(
        traceability.contains("8b1f410fc16ec4c867d28a1cd26c12fc495b8de5"),
        "traceability status must be tied to an explicit evaluated protected-main baseline"
    );
}

#[test]
fn required_architecture_decisions_are_indexed() {
    let index = read_required(&repository_root().join("docs/adr/README.md"));

    for adr in [
        "0014-api-and-event-contract-representation.md",
        "0015-persistence-and-transaction-boundaries.md",
        "0016-architecture-description-and-traceability.md",
        "0017-operational-recovery-and-ga-evidence.md",
        "0018-continuous-scores-and-narrative-separation.md",
        "0019-scientific-publication-evidence-gates.md",
    ] {
        assert!(index.contains(adr), "ADR index must expose {adr}");
    }
}
