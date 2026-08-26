//! Durable consent references reject the Unicode aliases refused by the Rust identity boundary.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_consent::apply_consent_migration;

fn client(schema: &str) -> Client {
    assert!(
        schema
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_'),
        "test schema names must remain fixed lowercase identifiers"
    );
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; \
             CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .unwrap();
    apply_consent_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_event(
    client: &mut Client,
    event_ref: &str,
    consent_form_version_ref: &str,
    research_scope_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    let purpose = if research_scope_ref.is_some() {
        "research_contribution"
    } else {
        "service_operation"
    };
    client.execute(
        "INSERT INTO consent_event (\
             participant_ref, event_ref, consent_purpose, consent_decision, \
             consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
         ) VALUES ('participant_default_ignorable_parity',$1,$2,'granted',$3,$4,1000)",
        &[
            &event_ref,
            &purpose,
            &consent_form_version_ref,
            &research_scope_ref,
        ],
    )
}

#[test]
fn every_consent_reference_field_rejects_default_ignorable_aliases() {
    let mut client = client("consent_default_ignorable_fields_test");
    let invalid_references = [
        "opaque_\u{00ad}_alpha",
        "opaque_\u{200b}_alpha",
        "opaque_\u{200d}_alpha",
        "opaque_\u{2060}_alpha",
        "opaque_\u{fe0f}_alpha",
        "opaque_\u{feff}_alpha",
        "opaque_\u{e0001}_alpha",
    ];

    for invalid_ref in invalid_references {
        let error = client
            .execute(
                "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
                &[&invalid_ref],
            )
            .expect_err("participant identity must reject a default-ignorable alias");
        assert_check(&error, "consent_ledger_participant_ref_format_check");
    }

    client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ('participant_default_ignorable_parity')",
            &[],
        )
        .unwrap();

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let error = insert_event(
            &mut client,
            invalid_ref,
            &format!("consent_form_event_{index}"),
            None,
        )
        .expect_err("event identity must reject a default-ignorable alias");
        assert_check(&error, "consent_event_event_ref_format_check");

        let error = insert_event(
            &mut client,
            &format!("consent_event_form_{index}"),
            invalid_ref,
            None,
        )
        .expect_err("consent-form identity must reject a default-ignorable alias");
        assert_check(&error, "consent_event_form_ref_format_check");

        let error = insert_event(
            &mut client,
            &format!("consent_event_scope_{index}"),
            &format!("consent_form_scope_{index}"),
            Some(invalid_ref),
        )
        .expect_err("research-scope identity must reject a default-ignorable alias");
        assert_check(&error, "consent_event_research_scope_format_check");
    }
}

#[test]
fn migration_reapplication_rejects_historical_default_ignorable_identity_without_rewriting_it() {
    let mut client = client("consent_default_ignorable_migration_test");
    client
        .batch_execute(
            "ALTER TABLE consent_ledger \
                 DROP CONSTRAINT consent_ledger_participant_ref_format_check; \
             ALTER TABLE consent_ledger \
                 ADD CONSTRAINT consent_ledger_participant_ref_format_check CHECK (participant_ref <> '');",
        )
        .unwrap();
    client
        .execute(
            "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
            &[&"participant_\u{200b}_historical"],
        )
        .expect("the deliberately weakened historical constraint must admit the regression row");

    let error = apply_consent_migration(&mut client)
        .expect_err("migration reapplication must fail closed on historical alias identity");
    let database_error = error
        .as_db_error()
        .expect("legacy-data preflight must return a PostgreSQL error");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database_error.message(),
        "consent reference migration blocked by legacy references outside the current opaque-reference contract"
    );

    let preserved: i64 = client
        .query_one(
            "SELECT count(*) FROM consent_ledger WHERE participant_ref = $1",
            &[&"participant_\u{200b}_historical"],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        preserved, 1,
        "migration must not rewrite immutable consent identity"
    );
}