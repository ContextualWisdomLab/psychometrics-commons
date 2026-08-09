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
fn coverage_failures_identify_the_incomplete_source_files() {
    assert!(CI_WORKFLOW.contains("INCOMPLETE_FILE"));
    assert!(CI_WORKFLOW.contains("entry.get(\"files\", [])"));
    assert!(CI_WORKFLOW.contains("summary.get(kind)"));
}

#[test]
fn line_coverage_failure_diagnostic_exposes_instantiation_gaps() {
    assert!(CI_WORKFLOW.contains(
        "cargo llvm-cov report --text --show-missing-lines --show-instantiations"
    ));
}

#[test]
fn branch_coverage_failure_diagnostic_uses_lcov_branch_records() {
    assert!(CI_WORKFLOW.contains(
        "cargo +nightly-2026-08-01 llvm-cov report --branch --lcov --output-path coverage-branches.lcov"
    ));
    assert!(CI_WORKFLOW.contains("raw_line.startswith(\"BRDA:\")"));
    assert!(CI_WORKFLOW.contains("taken in {\"0\", \"-\"}"));
}
