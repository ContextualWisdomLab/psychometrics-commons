//! Static regression contract for scoring-classification PostgreSQL fixture ownership.

const FIXTURE_SOURCE: &str = include_str!("postgres_scoring_job_classification_locking.rs");

#[test]
fn scoring_classification_fixture_uses_database_visible_serialization() {
    assert!(
        !FIXTURE_SOURCE.contains("Mutex::new(())"),
        "scoring classification fixture serialization must not be process-local"
    );
    assert!(
        FIXTURE_SOURCE.contains("SCORING_JOB_CLASSIFICATION_LOCK_KEY"),
        "scoring classification fixture must use a stable PostgreSQL advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("pg_advisory_lock"),
        "scoring classification fixture guard must be visible to other PostgreSQL sessions"
    );
}
