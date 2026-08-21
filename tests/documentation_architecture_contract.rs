//! Repository architecture-documentation fitness checks.
//!
//! These tests keep the required product/architecture/governance viewpoints
//! discoverable, prevent accepted ADRs from disappearing from the authoritative
//! index, and pin the cross-document vocabulary that distinguishes protected-main
//! evidence from active-PR and target architecture. They intentionally validate
//! repository structure and traceability markers, not Mermaid layout or full
//! semantic correctness; human architecture review remains required for those.

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
        "docs/THREAT_MODEL.md",
        "docs/TEST_STRATEGY.md",
        "docs/OPERABILITY.md",
        "docs/RELEASE_ACCEPTANCE.md",
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
        "docs/adr/0020-append-only-participant-identity-link-history.md",
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
        "docs/THREAT_MODEL.md",
        "docs/TEST_STRATEGY.md",
        "docs/OPERABILITY.md",
        "docs/RELEASE_ACCEPTANCE.md",
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
fn release_authority_separates_software_instrument_and_research_gates() {
    let release = read_required(&repository_root().join("docs/RELEASE_ACCEPTANCE.md"));
    for required_heading in [
        "Software release gate",
        "Consumer instrument release gate",
        "Research dataset release gate",
        "Release blockers",
        "Post-release verification",
    ] {
        assert!(
            release.contains(required_heading),
            "release acceptance must define {required_heading}"
        );
    }

    assert!(
        release.contains("Correlation alone is not sufficient"),
        "instrument release must reject correlation-only recovery claims"
    );
}

#[test]
fn threat_test_and_operability_docs_preserve_evidence_maturity() {
    let threat_model = read_required(&repository_root().join("docs/THREAT_MODEL.md"));
    let test_strategy = read_required(&repository_root().join("docs/TEST_STRATEGY.md"));
    let operability = read_required(&repository_root().join("docs/OPERABILITY.md"));

    for marker in [
        "architecture controls described here are not evidence",
        "cross-tenant IDOR/BOLA",
        "research re-identification",
        "outbox/inbox cross-tenant collision",
    ] {
        assert!(
            threat_model.contains(marker),
            "threat model must preserve marker {marker}"
        );
    }

    for marker in [
        "100% statement/branch coverage does not prove",
        "Correlation alone is not accepted",
        "real supported PostgreSQL version",
        "exact source/artifact it actually tested",
    ] {
        assert!(
            test_strategy.contains(marker),
            "test strategy must preserve marker {marker}"
        );
    }

    for marker in [
        "measured service levels remain evidence-gated",
        "single undifferentiated `healthy=true` is insufficient",
        "does **not** publish universal RPO/RTO numbers",
        "runbook link that has never been exercised is documentation, not recovery evidence",
    ] {
        assert!(
            operability.contains(marker),
            "operability contract must preserve marker {marker}"
        );
    }
}

#[test]
fn traceability_distinguishes_current_implementation_from_targets() {
    let traceability = read_required(&repository_root().join("docs/TRACEABILITY.md"));

    for status in [
        "Implemented",
        "Partially implemented",
        "Active PR",
        "Target",
        "External dependency",
    ] {
        assert!(
            traceability.contains(status),
            "traceability document must define status {status}"
        );
    }

    for protected_main_module in [
        "src/item_delivery.rs",
        "src/participant.rs",
        "src/authorization.rs",
        "src/integration.rs",
    ] {
        assert!(
            traceability.contains(protected_main_module),
            "traceability must reconcile protected-main module {protected_main_module}"
        );
    }

    let active_work = traceability
        .split("### Active implementation work that is not protected-main truth")
        .nth(1)
        .and_then(|section| section.split("\n## ").next())
        .expect("traceability must define the active implementation-work section");
    let pr_entry = active_work
        .lines()
        .find(|line| line.contains("**Active PR**"))
        .expect("active implementation work must contain an Active PR entry");
    assert!(
        pr_entry.contains("not protected-main truth") && !pr_entry.contains("**Implemented**"),
        "active work must remain explicitly segregated from protected-main truth"
    );

    let marker = "- Evaluated protected-main implementation baseline: `";
    let baseline_line = traceability
        .lines()
        .find(|line| line.starts_with(marker))
        .expect("traceability must name its evaluated protected-main baseline");
    let baseline = baseline_line
        .strip_prefix(marker)
        .and_then(|value| value.strip_suffix('`'))
        .expect("evaluated protected-main baseline must be enclosed in backticks");

    assert_eq!(
        baseline.len(),
        40,
        "evaluated protected-main baseline must be a full Git commit SHA"
    );
    assert!(
        baseline
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "evaluated protected-main baseline must be lowercase hexadecimal"
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
        "0020-append-only-participant-identity-link-history.md",
    ] {
        assert!(index.contains(adr), "ADR index must expose {adr}");
    }
}

#[test]
fn erd_covers_current_delivery_identity_and_longitudinal_boundaries() {
    let erd = read_required(&repository_root().join("docs/architecture/ERD.md"));

    for logical_entity in [
        "anonymous_credential_evidence",
        "item_delivery_event",
        "participant_identity_link",
        "longitudinal_enrollment",
        "longitudinal_observation_record",
        "temporal_analysis_submission",
    ] {
        assert!(
            erd.contains(logical_entity),
            "logical ERD must expose {logical_entity}"
        );
    }

    assert!(
        erd.contains("logical target ERD")
            && erd.contains("not the future mutable persistence source of truth"),
        "ERD must distinguish target logical persistence from current participant projection/as-built evidence"
    );
    for time_field in [
        "validity_start_at",
        "validity_end_at",
        "recorded_at",
        "received_at",
        "ingested_at",
    ] {
        assert!(
            erd.contains(time_field),
            "longitudinal ERD must preserve time field {time_field}"
        );
    }
}

#[test]
fn anonymous_credential_persistence_docs_match_active_schema() {
    let root = repository_root();
    let migration = read_required(&root.join("migrations/0020_anonymous_credential_evidence.sql"));
    let adr =
        read_required(&root.join("docs/adr/0003-keyverse-identity-and-anonymous-participation.md"));
    let erd = read_required(&root.join("docs/architecture/ERD.md"));
    let traceability = read_required(&root.join("docs/TRACEABILITY.md"));

    for required_schema_marker in [
        "anonymous_credential_evidence",
        "credential_ref",
        "tenant_ref",
        "participant_ref",
        "session_ref",
        "proof_digest",
        "issued_at_unix_ms",
        "expires_at_unix_ms",
        "revoked_at_unix_ms",
        "UNIQUE (proof_digest)",
        "BEFORE UPDATE OR DELETE",
        "BEFORE TRUNCATE",
    ] {
        assert!(
            migration.contains(required_schema_marker),
            "anonymous credential migration must preserve {required_schema_marker}"
        );
    }

    let lower_migration = migration.to_ascii_lowercase();
    for prohibited_secret_column in [
        "raw_bearer",
        "bearer_secret",
        "keyverse_subject",
        "research_participant",
        "research_pseudonym",
    ] {
        assert!(
            !lower_migration.contains(prohibited_secret_column),
            "anonymous credential persistence must not add prohibited identity/secret column {prohibited_secret_column}"
        );
    }

    for (label, document) in [
        ("ADR-0003", adr.as_str()),
        ("ERD", erd.as_str()),
        ("TRACEABILITY", traceability.as_str()),
    ] {
        assert!(
            document.contains("#302"),
            "{label} must identify #302 as active anonymous-credential persistence evidence"
        );
        assert!(
            document.contains("not protected-main truth")
                || document.contains("must not be promoted to protected-main truth")
                || document.contains("must become protected-main truth only after #302 merges"),
            "{label} must not promote active anonymous-credential persistence to shipped truth"
        );
    }

    assert!(
        erd.contains("migrations/0020_anonymous_credential_evidence.sql")
            && erd.contains("IMPLEMENTED_ON_ACTIVE_PR"),
        "ERD must connect the logical credential entity to its active-PR migration without claiming protected-main implementation"
    );
    assert!(
        adr.contains("Raw bearer proofs remain outside the product database")
            && traceability.contains("Raw anonymous bearer proof is not persisted"),
        "ADR and traceability must preserve the raw-bearer-proof exclusion"
    );
}

#[test]
fn anonymous_command_docs_do_not_claim_store_load() {
    let root = repository_root();
    let authorization = read_required(&root.join("src/anonymous_authorization.rs"));
    let security = read_required(&root.join("docs/architecture/SECURITY_AND_DATA.md"));
    let changelog = read_required(&root.join("CHANGELOG.md"));
    let traceability = read_required(&root.join("docs/TRACEABILITY.md"));
    let adr =
        read_required(&root.join("docs/adr/0003-keyverse-identity-and-anonymous-participation.md"));
    let erd = read_required(&root.join("docs/architecture/ERD.md"));
    let command_tests =
        read_required(&root.join("tests/anonymous_session_command_authorization.rs"));
    let uml = read_required(&root.join("docs/architecture/UML.md"));

    assert!(
        !authorization.contains("have been loaded from the product store"),
        "apply_anonymous_session_command rustdoc must not claim the caller already loaded records"
    );
    assert!(
        !authorization.contains("ParticipantRecord`] loaded from the product store"),
        "authorize_anonymous_session_command rustdoc must not label the participant argument as store-loaded"
    );
    assert!(
        authorization.contains("does not prove the records were loaded"),
        "command authorization rustdoc must say the gate does not prove store load"
    );
    assert!(
        !security.contains("supplied after a store load"),
        "SECURITY_AND_DATA must not claim the command gate observed a store load"
    );
    assert!(
        security.contains("does not prove those records were store-loaded"),
        "SECURITY_AND_DATA must say the command gate does not prove store load"
    );
    assert!(
        uml.contains(
            "as-built command gate compares supplied records and does not perform the load"
        ),
        "UML happy-path must distinguish target store load from the as-built command gate"
    );
    assert!(
        !uml.contains("authorize anonymous command from loaded records"),
        "UML command step must not say the gate authorized from loaded records"
    );
    assert!(
        uml.contains("authorize anonymous command from supplied records"),
        "UML command step must say the gate compares supplied records"
    );
    assert!(
        !authorization.contains("already loaded a participant"),
        "command authorization rustdoc must not assume the caller already loaded records"
    );
    assert!(
        !command_tests.contains("must load the participant"),
        "command tests must not say a transport must load records the gate does not prove"
    );
    assert!(
        !command_tests.contains("_loaded_"),
        "command tests must name supplied records, not loaded records"
    );

    for (label, document) in [
        ("CHANGELOG.md", changelog.as_str()),
        ("docs/TRACEABILITY.md", traceability.as_str()),
        (
            "docs/adr/0003-keyverse-identity-and-anonymous-participation.md",
            adr.as_str(),
        ),
        ("docs/architecture/ERD.md", erd.as_str()),
        ("docs/architecture/SECURITY_AND_DATA.md", security.as_str()),
        (
            "tests/anonymous_session_command_authorization.rs",
            command_tests.as_str(),
        ),
        ("src/anonymous_authorization.rs", authorization.as_str()),
    ] {
        assert!(
            !document.contains("remains Active PR #114"),
            "{label} must not name superseded #114 as the current participant persist landing"
        );
        assert!(
            !document.contains("remains Active PR #133"),
            "{label} must not name superseded #133 as the current participant persist landing"
        );
        assert!(
            !document.contains("remains Active PR #147"),
            "{label} must not name superseded #147 as the current participant persist landing"
        );
        assert!(
            !document.contains("Active PR #158"),
            "{label} must not name closed restore-reconcile #158 as the current participant persist landing"
        );
        let persist_status = document.to_ascii_lowercase();
        assert!(
            persist_status.contains("persist/reload remains target")
                || persist_status.contains("persist/reload of `assessment_participant` remains target"),
            "{label} must say assessment_participant persist/reload remains Target on this honesty head"
        );
    }
}

#[test]
fn uml_covers_identity_longitudinal_and_workbench_behavior() {
    let uml = read_required(&repository_root().join("docs/architecture/UML.md"));

    for behavior_marker in [
        "Participant identity-link lifecycle",
        "Longitudinal Gyeot-to-TEPP orchestration sequence",
        "Measurement Workbench publication-evidence sequence",
        "ItemDeliveryEvent",
        "ParticipantIdentityLink",
    ] {
        assert!(
            uml.contains(behavior_marker),
            "UML architecture view must expose {behavior_marker}"
        );
    }
}
