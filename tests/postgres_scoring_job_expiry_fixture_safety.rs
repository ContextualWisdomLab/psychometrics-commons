//! Failure-injection fixture safety for scoring-job lease-expiry `PostgreSQL` tests.

#[test]
fn expiry_failure_injection_does_not_create_process_named_schema() {
    let source = include_str!("postgres_scoring_job_lease_expiry_recovery.rs");

    assert!(
        !source.contains("std::process::id()"),
        "failure-injection namespaces must not depend on recyclable process IDs"
    );
    assert!(
        !source.contains("CREATE SCHEMA {sink}"),
        "failure injection must not create an auxiliary schema that outlives the assertion"
    );
    assert!(
        source.contains("scoring_job_expiry_classify_sink_unavailable"),
        "failure injection must use a deliberately unavailable namespace"
    );
}
