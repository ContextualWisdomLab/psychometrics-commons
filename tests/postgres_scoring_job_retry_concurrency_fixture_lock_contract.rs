//! Regression contract for the scoring retry-concurrency PostgreSQL fixture lock.

#[test]
fn normal_fixture_guard_configures_finite_lock_timeout_before_advisory_wait() {
    let source = include_str!("postgres_scoring_job_retry_concurrency.rs");
    let timeout = source
        .find("SET lock_timeout = '60s'")
        .expect("normal fixture guard must bound advisory-lock acquisition");
    let lock = source
        .find("SELECT pg_advisory_lock($1)")
        .expect("fixture must acquire the PostgreSQL advisory lock");

    assert!(
        timeout < lock,
        "lock_timeout must be configured before blocking on the fixture advisory lock"
    );
}
