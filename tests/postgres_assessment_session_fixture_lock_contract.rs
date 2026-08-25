//! Static regression contract for assessment-session `PostgreSQL` fixture ownership.
//!
//! This test intentionally reads the integration-test source so a process-local
//! mutex cannot silently return after the fixture is made database-visible.

const FIXTURE_SOURCE: &str = include_str!("postgres_assessment_session_persistence.rs");

#[test]
fn assessment_session_fixture_uses_database_visible_serialization() {
    assert!(
        !FIXTURE_SOURCE.contains("Mutex::new(())"),
        "assessment-session fixture serialization must not be process-local"
    );
    assert!(
        FIXTURE_SOURCE.contains("ASSESSMENT_SESSION_PERSISTENCE_LOCK_KEY"),
        "assessment-session fixture must use a stable PostgreSQL advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("pg_advisory_lock"),
        "assessment-session fixture guard must be visible to other PostgreSQL sessions"
    );
}
