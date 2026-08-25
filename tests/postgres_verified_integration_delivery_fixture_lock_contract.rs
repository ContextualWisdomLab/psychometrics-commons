//! Regression contract for the verified-integration-delivery PostgreSQL fixture lock.

#[test]
fn ready_client_bounds_fixture_lock_wait_before_advisory_acquisition() {
    let source = include_str!("postgres_verified_integration_delivery.rs");
    let timeout = source
        .find("SET lock_timeout = '60s'")
        .expect("verified handoff fixture must bound advisory-lock acquisition");
    let lock = source
        .find("SELECT pg_advisory_lock($1)")
        .expect("verified handoff fixture must acquire a PostgreSQL advisory lock");

    assert!(
        timeout < lock,
        "lock_timeout must be configured before blocking on the fixture advisory lock"
    );
}
