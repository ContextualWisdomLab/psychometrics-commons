//! Real `PostgreSQL` bounds for durable instrument-release rows.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_instrument_release::apply_instrument_release_migration;

const INSTRUMENT_RELEASE_SCHEMA_DATABASE_LOCK_KEY: i64 = 0x494E_5354_5253_4348;

fn test_clients() -> (Client, Client) {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut guard = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    guard
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&INSTRUMENT_RELEASE_SCHEMA_DATABASE_LOCK_KEY],
        )
        .expect("PostgreSQL fixture advisory lock should be acquired");

    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS instrument_release_schema_test;\
             SET search_path TO instrument_release_schema_test;",
        )
        .unwrap();
    (guard, client)
}

fn reset_schema(client: &mut Client) {
    client
        .batch_execute("DROP TABLE IF EXISTS instrument_release_schema_test.instrument_release;")
        .unwrap();
}

fn constraint_name(error: &postgres::Error) -> String {
    error
        .as_db_error()
        .and_then(postgres::error::DbError::constraint)
        .unwrap_or_default()
        .to_owned()
}

const VALID_COLUMNS: &str = "release_ref, instrument_ref, instrument_version_ref, construct_ref, \
     item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
     calibration_reference, norm_version_ref, narrative_version_ref, \
     consent_requirement_refs, intended_use_ref, limitations_ref, \
     content_digest, publication_state, created_at_unix_ms";

const VALID_VALUES: &str = "'release_schema_ko_v1', 'instrument_big_five', \
     'instrument_version_big_five_ko_v1', 'construct_big_five', \
     ARRAY['item_version_001'], 'ko-KR', 'assessment_spec_big_five_v1', \
     'scoring_version_big_five_v1', 'calibration_big_five_ko_v1', \
     'norm_version_big_five_ko_v1', 'narrative_version_big_five_v1', \
     ARRAY['consent_service_v1'], 'intended_use_self_reflection_v1', \
     'limitations_nonclinical_v1', \
     'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef', \
     'draft', 40000";

#[test]
fn fixture_lock_is_visible_across_database_sessions() {
    let (_guard, _owner) = test_clients();
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut contender = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    let acquired: bool = contender
        .query_one(
            "SELECT pg_try_advisory_lock($1)",
            &[&INSTRUMENT_RELEASE_SCHEMA_DATABASE_LOCK_KEY],
        )
        .unwrap()
        .get(0);

    if acquired {
        contender
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&INSTRUMENT_RELEASE_SCHEMA_DATABASE_LOCK_KEY],
            )
            .unwrap();
    }

    assert!(
        !acquired,
        "fixture serialization must be enforced by PostgreSQL, not only by a process-local mutex"
    );
}

#[test]
fn schema_rejects_numeric_identity_empty_item_set_invalid_digest_and_unknown_state() {
    let (_guard, mut client) = test_clients();
    reset_schema(&mut client);
    apply_instrument_release_migration(&mut client).unwrap();

    let numeric = client
        .execute(
            &format!(
                "INSERT INTO instrument_release ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace("'release_schema_ko_v1'", "'12'")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&numeric),
        "instrument_release_release_ref_format_check"
    );

    let empty_items = client
        .execute(
            &format!(
                "INSERT INTO instrument_release ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace("ARRAY['item_version_001']", "ARRAY[]::TEXT[]")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&empty_items),
        "instrument_release_item_refs_not_empty_check"
    );

    let digest = client
        .execute(
            &format!(
                "INSERT INTO instrument_release ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace(
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    "not-a-digest"
                )
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&digest),
        "instrument_release_digest_format_check"
    );

    let state = client
        .execute(
            &format!(
                "INSERT INTO instrument_release ({VALID_COLUMNS}) VALUES ({})",
                VALID_VALUES.replace("'draft'", "'archived'")
            ),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        constraint_name(&state),
        "instrument_release_state_value_check"
    );
}
