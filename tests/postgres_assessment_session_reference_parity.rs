//! PostgreSQL assessment-session identity must match the Rust opaque-reference boundary.
//!
//! Session headers and append-only command history are durable lifecycle provenance. Direct SQL
//! must not persist Unicode-numeric aliases, surrounding Unicode whitespace, embedded controls,
//! or default-ignorable code points that `normalized_reference` rejects or would normalize in the
//! Rust domain.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_assessment_session::apply_assessment_session_migration;
use std::sync::{Mutex, MutexGuard};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn client() -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS assessment_session_reference_parity_test CASCADE; \
             CREATE SCHEMA assessment_session_reference_parity_test; \
             SET search_path TO assessment_session_reference_parity_test;",
        )
        .unwrap();
    apply_assessment_session_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

fn insert_session(
    client: &mut Client,
    session_ref: &str,
    participant_ref: &str,
    instrument_release_ref: &str,
    instrument_version_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO assessment_session (\
             session_ref, participant_ref, instrument_release_ref, instrument_version_ref, \
             instrument_release_content_digest, locale, session_state, created_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,$5,'ko-KR','created',40000)",
        &[
            &session_ref,
            &participant_ref,
            &instrument_release_ref,
            &instrument_version_ref,
            &RELEASE_DIGEST,
        ],
    )
}

fn insert_command(
    client: &mut Client,
    session_ref: &str,
    command_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO assessment_session_command (\
             session_ref, command_ref, command_sequence, command_name, resulting_state\
         ) VALUES ($1,$2,1,'activate','active')",
        &[&session_ref, &command_ref],
    )
}

#[derive(Clone, Copy, Debug)]
enum SessionReferenceField {
    Session,
    Participant,
    InstrumentRelease,
    InstrumentVersion,
}

fn assert_session_field_rejects_invalid_aliases(
    client: &mut Client,
    field: SessionReferenceField,
    constraint: &str,
) {
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
        "opaque_\u{00ad}_alpha",
        "opaque_\u{200b}_alpha",
        "opaque_\u{200d}_alpha",
        "opaque_\u{2060}_alpha",
        "opaque_\u{fe0f}_alpha",
        "opaque_\u{feff}_alpha",
        "opaque_\u{e0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let suffix = format!("{}_{}", field as u8, index);
        let mut session_ref = format!("session_{suffix}");
        let mut participant_ref = format!("participant_{suffix}");
        let mut release_ref = format!("release_{suffix}");
        let mut version_ref = format!("instrument_version_{suffix}");
        match field {
            SessionReferenceField::Session => session_ref = invalid_ref.to_owned(),
            SessionReferenceField::Participant => participant_ref = invalid_ref.to_owned(),
            SessionReferenceField::InstrumentRelease => release_ref = invalid_ref.to_owned(),
            SessionReferenceField::InstrumentVersion => version_ref = invalid_ref.to_owned(),
        }

        let error = insert_session(
            client,
            &session_ref,
            &participant_ref,
            &release_ref,
            &version_ref,
        )
        .expect_err("direct SQL must not bypass the Rust session-reference boundary");
        assert_check(&error, constraint);
    }
}

#[test]
fn every_session_header_reference_rejects_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();

    for (field, constraint) in [
        (
            SessionReferenceField::Session,
            "assessment_session_session_ref_format_check",
        ),
        (
            SessionReferenceField::Participant,
            "assessment_session_participant_ref_format_check",
        ),
        (
            SessionReferenceField::InstrumentRelease,
            "assessment_session_release_ref_format_check",
        ),
        (
            SessionReferenceField::InstrumentVersion,
            "assessment_session_version_ref_format_check",
        ),
    ] {
        assert_session_field_rejects_invalid_aliases(&mut client, field, constraint);
    }
}

#[test]
fn command_reference_uses_the_same_opaque_identity_boundary() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_ref) in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}command_alpha",
        "command_\u{0001}_alpha",
        "command_\u{00ad}_alpha",
        "command_\u{200b}_alpha",
        "command_\u{200d}_alpha",
        "command_\u{2060}_alpha",
        "command_\u{fe0f}_alpha",
        "command_\u{feff}_alpha",
        "command_\u{e0001}_alpha",
    ]
    .into_iter()
    .enumerate()
    {
        let session_ref = format!("session_command_{index}");
        insert_session(
            &mut client,
            &session_ref,
            &format!("participant_command_{index}"),
            &format!("release_command_{index}"),
            &format!("instrument_version_command_{index}"),
        )
        .unwrap();
        let error = insert_command(&mut client, &session_ref, invalid_ref)
            .expect_err("command identity must match normalized_reference");
        assert_check(
            &error,
            "assessment_session_command_command_ref_format_check",
        );
    }
}

#[test]
fn migration_reapplication_restores_weakened_session_and_command_constraints() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE assessment_session \
                 DROP CONSTRAINT assessment_session_session_ref_format_check; \
             ALTER TABLE assessment_session \
                 ADD CONSTRAINT assessment_session_session_ref_format_check CHECK (\
                     session_ref = btrim(session_ref) AND session_ref <> ''\
                 ); \
             ALTER TABLE assessment_session_command \
                 DROP CONSTRAINT assessment_session_command_command_ref_format_check; \
             ALTER TABLE assessment_session_command \
                 ADD CONSTRAINT assessment_session_command_command_ref_format_check CHECK (\
                     command_ref = btrim(command_ref) AND command_ref <> ''\
                 );",
        )
        .unwrap();

    apply_assessment_session_migration(&mut client).unwrap();

    let error = insert_session(
        &mut client,
        "½",
        "participant_upgrade_guard",
        "release_upgrade_guard",
        "instrument_version_upgrade_guard",
    )
    .expect_err("migration reapplication must restore the stronger session predicate");
    assert_check(&error, "assessment_session_session_ref_format_check");

    insert_session(
        &mut client,
        "session_upgrade_command",
        "participant_upgrade_command",
        "release_upgrade_command",
        "instrument_version_upgrade_command",
    )
    .unwrap();
    let error = insert_command(&mut client, "session_upgrade_command", "½")
        .expect_err("migration reapplication must restore the stronger command predicate");
    assert_check(
        &error,
        "assessment_session_command_command_ref_format_check",
    );
}

#[test]
fn migration_reapplication_fails_closed_on_historical_invalid_session_identity() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE assessment_session \
                 DROP CONSTRAINT assessment_session_session_ref_format_check; \
             ALTER TABLE assessment_session \
                 ADD CONSTRAINT assessment_session_session_ref_format_check CHECK (\
                     session_ref = btrim(session_ref) AND session_ref <> ''\
                 );",
        )
        .unwrap();

    insert_session(
        &mut client,
        "½",
        "participant_historical",
        "release_historical",
        "instrument_version_historical",
    )
    .expect("weakened historical predicate must admit the regression fixture");

    let error = apply_assessment_session_migration(&mut client)
        .expect_err("upgrade must fail closed instead of blessing invalid session identity");
    assert_check(&error, "assessment_session_session_ref_format_check");
}
