//! Failure-injection fixture safety for scoring-job lease-expiry `PostgreSQL` tests.

#[test]
fn expiry_failure_injection_uses_only_the_declared_unavailable_sink() {
    let source = include_str!("postgres_scoring_job_lease_expiry_recovery.rs");
    let (_, target_and_rest) = source
        .split_once("fn expiry_classify_select_failure_is_a_database_failure()")
        .expect("lease-expiry failure-injection test must exist");
    let target = target_and_rest
        .split_once("\n#[test]\n")
        .map_or(target_and_rest, |(body, _)| body);

    assert!(
        target.contains(
            "const SINK: &str = \"scoring_job_expiry_classify_sink_unavailable\";"
        ),
        "failure injection must declare the deliberately unavailable sink namespace"
    );
    assert!(
        target.contains("PERFORM set_config('search_path', '{SINK}', false);"),
        "failure injection must redirect search_path to its declared SINK namespace"
    );
    assert!(
        !target.contains("CREATE SCHEMA"),
        "failure injection must not create its unavailable sink or any auxiliary schema"
    );
    assert!(
        !target.contains("std::process::id()"),
        "failure-injection namespaces must not depend on recyclable process IDs"
    );
}
