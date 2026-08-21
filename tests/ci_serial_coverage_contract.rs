//! Regression contract for the serialized coverage runner.
//!
//! The combined runner must preserve independent branch-coverage evidence even
//! when the line-coverage gate has already failed. This keeps the two-runner
//! allocation remedy from hiding a second coverage defect in the same run.

const CI_WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");

#[test]
fn branch_coverage_still_runs_after_line_coverage_failure() {
    assert!(CI_WORKFLOW.contains(
        "- name: Generate branch coverage\n        id: branch_coverage_generation\n        if: ${{ !cancelled() }}"
    ));
    assert!(CI_WORKFLOW.contains(
        "- name: Enforce complete branch coverage\n        id: branch_coverage_gate\n        if: ${{ !cancelled() && steps.branch_coverage_generation.outcome == 'success' }}"
    ));
}
