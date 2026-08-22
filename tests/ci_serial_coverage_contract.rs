//! Regression contract for capacity-aware coverage scheduling.
//!
//! Runtime CI must reduce peak hosted-runner pressure without renaming the two
//! long-lived coverage check identities. Branch coverage therefore waits for
//! the line-coverage job, but still runs after a line-coverage failure unless
//! the workflow was cancelled. Generation failures remain explicit operator
//! diagnostics rather than silently skipped evidence.

const CI_WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");

#[test]
fn coverage_jobs_preserve_their_check_identities() {
    assert!(CI_WORKFLOW.contains("\n  line-coverage:\n    name: Production line coverage\n"));
    assert!(CI_WORKFLOW.contains("\n  branch-coverage:\n    name: Production branch coverage\n"));
    assert!(!CI_WORKFLOW.contains("name: Production line and branch coverage"));
}

#[test]
fn branch_coverage_is_serialized_without_becoming_fail_open() {
    assert!(CI_WORKFLOW.contains(
        "\n  branch-coverage:\n    name: Production branch coverage\n    needs: line-coverage\n    if: ${{ always() && !cancelled() }}\n"
    ));
    assert_eq!(CI_WORKFLOW.matches("runs-on: ubuntu-latest").count(), 3);
}

#[test]
fn line_generation_failure_has_operator_diagnostics() {
    assert!(CI_WORKFLOW
        .contains("- name: Generate line coverage\n        id: line_coverage_generation"));
    assert!(CI_WORKFLOW.contains(
        "- name: Diagnose line coverage generation failure\n        if: ${{ !cancelled() && steps.line_coverage_generation.outcome == 'failure' }}"
    ));
    assert!(
        CI_WORKFLOW.contains("::error::line coverage generation failed before the coverage gate")
    );
}

#[test]
fn branch_generation_failure_has_operator_diagnostics() {
    assert!(CI_WORKFLOW
        .contains("- name: Generate branch coverage\n        id: branch_coverage_generation"));
    assert!(CI_WORKFLOW.contains(
        "- name: Diagnose branch coverage generation failure\n        if: ${{ !cancelled() && steps.branch_coverage_generation.outcome == 'failure' }}"
    ));
    assert!(
        CI_WORKFLOW.contains("::error::branch coverage generation failed before the coverage gate")
    );
}
