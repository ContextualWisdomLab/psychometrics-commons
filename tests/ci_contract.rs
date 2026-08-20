//! Integration tests for repository CI evidence semantics.

const CI_WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");
const RUST_TOOLCHAIN: &str = include_str!("../rust-toolchain.toml");
const DEPENDABOT: &str = include_str!("../.github/dependabot.yml");

#[test]
fn every_checkout_is_bound_to_the_pull_request_head() {
    let exact_head_ref = "ref: ${{ github.event.pull_request.head.sha || github.sha }}";
    assert_eq!(CI_WORKFLOW.matches(exact_head_ref).count(), 3);
}

#[test]
fn every_checkout_drops_persisted_credentials() {
    assert_eq!(CI_WORKFLOW.matches("persist-credentials: false").count(), 3);
}

#[test]
fn postgres_service_image_is_immutably_pinned() {
    const PINNED_IMAGE: &str =
        "image: postgres:18-alpine@sha256:9a8afca54e7861fd90fab5fdf4c42477a6b1cb7d293595148e674e0a3181de15";
    assert_eq!(CI_WORKFLOW.matches(PINNED_IMAGE).count(), 3);

    let postgres_images: Vec<_> = CI_WORKFLOW
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("image: postgres"))
        .collect();
    assert_eq!(postgres_images.len(), 3);
    assert!(postgres_images
        .iter()
        .all(|image| image.contains("@sha256:")));
}

#[test]
fn postgres_health_checks_allow_initialization_restart() {
    assert_eq!(CI_WORKFLOW.matches("--health-start-period 5s").count(), 3);
}

#[test]
fn postgres_credentials_are_ephemeral_per_workflow_run() {
    const EPHEMERAL_PASSWORD: &str =
        "POSTGRES_PASSWORD: ci_${{ github.run_id }}_${{ github.run_attempt }}";

    assert!(!CI_WORKFLOW.contains("POSTGRES_PASSWORD: postgres"));
    assert!(!CI_WORKFLOW.contains("password=postgres"));
    assert_eq!(CI_WORKFLOW.matches(EPHEMERAL_PASSWORD).count(), 3);
    assert_eq!(
        CI_WORKFLOW
            .matches("name: Configure ephemeral PostgreSQL test connection")
            .count(),
        3
    );
}

#[test]
fn compile_gate_rejects_stale_lockfiles_before_building() {
    assert!(CI_WORKFLOW.contains("run: cargo check --locked --all-targets"));
}

#[test]
fn lockfile_artifact_upload_uses_node24_compatible_action() {
    const NODE24_UPLOAD: &str =
        "uses: actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f";
    assert!(CI_WORKFLOW.contains(NODE24_UPLOAD));
    assert!(!CI_WORKFLOW
        .contains("uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02"));
}

#[test]
fn lockfile_failure_evidence_is_scoped_to_the_lock_gate() {
    assert!(CI_WORKFLOW.contains("id: cargo_lock_gate"));
    assert_eq!(
        CI_WORKFLOW
            .matches("if: failure() && steps.cargo_lock_gate.outcome == 'failure'")
            .count(),
        2
    );
}

#[test]
fn coverage_failures_identify_the_incomplete_source_files() {
    assert!(CI_WORKFLOW.contains("INCOMPLETE_FILE"));
    assert!(CI_WORKFLOW.contains("entry.get(\"files\", [])"));
    assert!(CI_WORKFLOW.contains("summary.get(kind)"));
}

#[test]
fn line_coverage_failure_diagnostic_exposes_instantiation_gaps() {
    assert!(CI_WORKFLOW
        .contains("cargo llvm-cov report --text --show-missing-lines --show-instantiations"));
}

#[test]
fn line_coverage_failure_diagnostic_emits_machine_readable_annotations() {
    assert!(CI_WORKFLOW.contains("cargo llvm-cov report --lcov --output-path coverage-lines.lcov"));
    assert!(CI_WORKFLOW.contains("raw_line.startswith(\"DA:\")"));
    assert!(CI_WORKFLOW.contains("hits == \"0\""));
    assert!(CI_WORKFLOW.contains("::error file={source},line={line}::uncovered production line"));
}

#[test]
fn branch_coverage_failure_diagnostic_uses_lcov_branch_records() {
    assert!(CI_WORKFLOW.contains(
        "cargo +nightly-2026-08-18 llvm-cov report --branch --lcov --output-path coverage-branches.lcov"
    ));
    assert!(CI_WORKFLOW.contains("raw_line.startswith(\"BRDA:\")"));
    assert!(CI_WORKFLOW.contains("taken in {\"0\", \"-\"}"));
}

#[test]
fn rust_toolchains_are_exact_and_reviewably_updated() {
    const STABLE_QUALITY_INSTALL: &str =
        "rustup toolchain install 1.97.1 --profile minimal --component clippy --component rustfmt";
    const STABLE_COVERAGE_INSTALL: &str =
        "rustup toolchain install 1.97.1 --profile minimal --component llvm-tools-preview";
    const NIGHTLY_COVERAGE_INSTALL: &str =
        "rustup toolchain install nightly-2026-08-18 --profile minimal --component llvm-tools-preview";
    const NIGHTLY_LLVM_COV_INSTALL: &str =
        "cargo +nightly-2026-08-18 install cargo-llvm-cov --locked --version \"$CARGO_LLVM_COV_VERSION\"";
    const NIGHTLY_LLVM_COV_VERSION: &str =
        "cargo +nightly-2026-08-18 llvm-cov --version | grep -F \"$CARGO_LLVM_COV_VERSION\"";
    const NIGHTLY_BRANCH_JSON: &str =
        "cargo +nightly-2026-08-18 llvm-cov --branch --json --summary-only --output-path coverage-branches.json";

    assert!(RUST_TOOLCHAIN.contains("channel = \"1.97.1\""));
    assert!(!RUST_TOOLCHAIN.contains("channel = \"stable\""));
    assert!(CI_WORKFLOW.contains(STABLE_QUALITY_INSTALL));
    assert!(CI_WORKFLOW.contains(STABLE_COVERAGE_INSTALL));
    assert!(CI_WORKFLOW.contains(NIGHTLY_COVERAGE_INSTALL));
    assert!(CI_WORKFLOW.contains(NIGHTLY_LLVM_COV_INSTALL));
    assert!(CI_WORKFLOW.contains(NIGHTLY_LLVM_COV_VERSION));
    assert!(CI_WORKFLOW.contains(NIGHTLY_BRANCH_JSON));
    assert!(!CI_WORKFLOW.contains("nightly-2026-08-01"));

    assert!(DEPENDABOT.contains("package-ecosystem: \"rust-toolchain\""));
    assert!(DEPENDABOT.contains("directory: \"/\""));
    assert!(DEPENDABOT.contains("interval: \"weekly\""));
}