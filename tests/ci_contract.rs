//! Integration tests for repository CI evidence semantics.

const CI_WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");

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
    assert_eq!(
        CI_WORKFLOW.matches("image: postgres:18-alpine\n").count(),
        0
    );
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
        "cargo +nightly-2026-08-01 llvm-cov report --branch --lcov --output-path coverage-branches.lcov"
    ));
    assert!(CI_WORKFLOW.contains("raw_line.startswith(\"BRDA:\")"));
    assert!(CI_WORKFLOW.contains("taken in {\"0\", \"-\"}"));
}
