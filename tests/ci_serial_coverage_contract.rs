//! Regression contract for the serialized coverage runner.
//!
//! The combined runner must preserve independent branch-coverage evidence even
//! when the line-coverage gate has already failed. It must also keep generation
//! failures visible as operator diagnostics instead of silently skipping the
//! corresponding gate. This keeps the two-runner allocation remedy from hiding
//! a second coverage defect in the same run.

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

#[test]
fn line_generation_failure_has_operator_diagnostics() {
    assert!(CI_WORKFLOW.contains(
        "- name: Generate line coverage\n        id: line_coverage_generation"
    ));
    assert!(CI_WORKFLOW.contains(
        "- name: Diagnose line coverage generation failure\n        if: ${{ !cancelled() && steps.line_coverage_generation.outcome == 'failure' }}"
    ));
    assert!(CI_WORKFLOW.contains(
        "::error::line coverage generation failed before the coverage gate"
    ));
}

#[test]
fn branch_generation_failure_has_operator_diagnostics() {
    assert!(CI_WORKFLOW.contains(
        "- name: Diagnose branch coverage generation failure\n        if: ${{ !cancelled() && steps.branch_coverage_generation.outcome == 'failure' }}"
    ));
    assert!(CI_WORKFLOW.contains(
        "::error::branch coverage generation failed before the coverage gate"
    ));
}
