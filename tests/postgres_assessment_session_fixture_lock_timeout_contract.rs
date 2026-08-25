//! Regression contract for the assessment-session PostgreSQL fixture lock.

const FIXTURE_SOURCE: &str = include_str!("postgres_assessment_session_persistence.rs");

#[test]
fn fixture_lock_wait_is_bounded_before_advisory_lock_acquisition() {
    let fixture_start = FIXTURE_SOURCE
        .find("fn assessment_session_test_guard() -> Client")
        .expect("the assessment-session persistence fixture must expose its database guard");
    let fixture_end = FIXTURE_SOURCE[fixture_start..]
        .find("fn test_client() -> (Client, Client)")
        .map(|offset| fixture_start + offset)
        .expect(
            "the assessment-session persistence fixture guard must end before test-client setup",
        );
    let guard_setup = &FIXTURE_SOURCE[fixture_start..fixture_end];

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
