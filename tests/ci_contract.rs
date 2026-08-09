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
