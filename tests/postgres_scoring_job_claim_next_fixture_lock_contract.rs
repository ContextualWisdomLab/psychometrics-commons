//! Regression contract for the scoring claim-next PostgreSQL fixture lock.

const FIXTURE_SOURCE: &str = include_str!("postgres_scoring_job_claim_next.rs");

#[test]
fn fixture_lock_wait_is_bounded_before_advisory_lock_acquisition() {
    let guard_start = FIXTURE_SOURCE
        .find("fn claim_next_test_guard() -> Client")
        .expect("the claim-next fixture must expose its database guard");
    let client_start = FIXTURE_SOURCE
        .find("fn test_client() -> Client")
        .expect("the claim-next fixture must expose its schema-scoped client");
    let guard_body = &FIXTURE_SOURCE[guard_start..client_start];

    let timeout_index = guard_body
        .find("lock_timeout")
        .expect("the PostgreSQL fixture guard must bound advisory-lock waits");
    let lock_index = guard_body
        .find("pg_advisory_lock")
        .expect("the PostgreSQL fixture guard must acquire its advisory lock");

    assert!(
        timeout_index < lock_index,
        "lock_timeout must be configured before waiting for the fixture advisory lock"
    );
    assert!(
        guard_body.contains("60s"),
        "normal fixture acquisition must use the repository-standard finite 60-second lock wait"
    );
}
