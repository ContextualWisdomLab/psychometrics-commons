//! Regression contract for the scoring classification PostgreSQL fixture lock.

const FIXTURE_SOURCE: &str = include_str!("postgres_scoring_job_classification_locking.rs");

#[test]
fn fixture_lock_wait_is_bounded_before_advisory_lock_acquisition() {
    let fixture_start = FIXTURE_SOURCE
        .find("fn test_clients() -> (Client, Client, Client)")
        .expect("the scoring classification fixture must expose its database clients");
    let owner_start = FIXTURE_SOURCE[fixture_start..]
        .find("let mut owner")
        .map(|offset| fixture_start + offset)
        .expect("the scoring classification fixture must create its owner client after the guard");
    let guard_setup = &FIXTURE_SOURCE[fixture_start..owner_start];

    let timeout_index = guard_setup
        .find("lock_timeout")
        .expect("the PostgreSQL fixture guard must bound advisory-lock waits");
    let lock_index = guard_setup
        .find("pg_advisory_lock")
        .expect("the PostgreSQL fixture guard must acquire its advisory lock");

    assert!(
        timeout_index < lock_index,
        "lock_timeout must be configured before waiting for the fixture advisory lock"
    );
    assert!(
        guard_setup.contains("60s"),
        "normal fixture acquisition must use the repository-standard finite 60-second lock wait"
    );
}
