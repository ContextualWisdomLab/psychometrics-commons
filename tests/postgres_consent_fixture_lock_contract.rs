//! Static regression contract for consent PostgreSQL fixture ownership.

const FIXTURE_SOURCE: &str = include_str!("postgres_consent_persistence.rs");

#[test]
fn consent_fixture_uses_database_visible_serialization() {
    assert!(
        !FIXTURE_SOURCE.contains("Mutex::new(())"),
        "consent persistence fixture serialization must not be process-local"
    );
    assert!(
        FIXTURE_SOURCE.contains("CONSENT_PERSISTENCE_LOCK_KEY"),
        "consent persistence fixture must use a stable PostgreSQL advisory-lock identity"
    );
    assert!(
        FIXTURE_SOURCE.contains("pg_advisory_lock"),
        "consent persistence fixture guard must be visible to other PostgreSQL sessions"
    );
}
