//! `PostgreSQL` scoring-request references must match the Rust opaque-reference boundary.
//!
//! The product domain trims Unicode whitespace and rejects embedded control characters and
//! numeric-like spellings under Rust `char::is_numeric`. Direct SQL and migration reapplication
//! must not leave durable scoring evidence that the Rust domain would reject or normalize.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_scoring_request::apply_scoring_request_migration;
use std::sync::{Mutex, MutexGuard};

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
            "DROP SCHEMA IF EXISTS scoring_request_reference_parity_test CASCADE; \
             CREATE SCHEMA scoring_request_reference_parity_test; \
             SET search_path TO scoring_request_reference_parity_test;",
        )
        .unwrap();
    apply_scoring_request_migration(&mut client).unwrap();
    client
}

fn assert_check(error: &postgres::Error, constraint: &str) {
    let database_error = error
        .as_db_error()
        .expect("reference rejection must come from a PostgreSQL CHECK constraint");
    assert_eq!(database_error.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(database_error.constraint(), Some(constraint));
}

#[allow(clippy::too_many_arguments)]
fn insert_request(
    client: &mut Client,
    scoring_request_ref: &str,
    session_ref: &str,
    response_snapshot_ref: &str,
    assessment_spec_ref: &str,
    instrument_version_ref: &str,
    scoring_version_ref: &str,
    calibration_reference: &str,
    norm_version_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO scoring_request (\
             scoring_request_ref, session_ref, response_snapshot_ref, assessment_spec_ref, \
             instrument_version_ref, scoring_version_ref, calibration_reference, \
             norm_version_ref, requested_output_schema_version\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1)",
        &[
            &scoring_request_ref,
            &session_ref,
            &response_snapshot_ref,
            &assessment_spec_ref,
            &instrument_version_ref,
            &scoring_version_ref,
            &calibration_reference,
            &norm_version_ref,
        ],
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReferenceField {
    Request,
    Session,
    Snapshot,
    AssessmentSpec,
    InstrumentVersion,
    ScoringVersion,
    Calibration,
    NormVersion,
}

fn assert_field_rejects_rust_invalid_aliases(
    client: &mut Client,
    field: ReferenceField,
    constraint: &str,
) {
    let invalid_references = [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let suffix = format!("{}_{}", field as u8, index);
        let mut request_ref = format!("scoring_request_{suffix}");
        let mut session_ref = format!("session_{suffix}");
        let mut snapshot_ref = format!("response_snapshot_{suffix}");
        let mut assessment_spec_ref = format!("assessment_spec_{suffix}");
        let mut instrument_version_ref = format!("instrument_version_{suffix}");
        let mut scoring_version_ref = format!("scoring_version_{suffix}");
        let mut calibration_reference = format!("calibration_reference_{suffix}");
        let norm_version_ref = format!("norm_version_{suffix}");

        match field {
            ReferenceField::Request => invalid_ref.clone_into(&mut request_ref),
            ReferenceField::Session => invalid_ref.clone_into(&mut session_ref),
            ReferenceField::Snapshot => invalid_ref.clone_into(&mut snapshot_ref),
            ReferenceField::AssessmentSpec => invalid_ref.clone_into(&mut assessment_spec_ref),
            ReferenceField::InstrumentVersion => {
                invalid_ref.clone_into(&mut instrument_version_ref);
            }
            ReferenceField::ScoringVersion => invalid_ref.clone_into(&mut scoring_version_ref),
            ReferenceField::Calibration => invalid_ref.clone_into(&mut calibration_reference),
            ReferenceField::NormVersion => {}
        }
        let norm_version = if field == ReferenceField::NormVersion {
            Some(invalid_ref)
        } else {
            Some(norm_version_ref.as_str())
        };

        let error = insert_request(
            client,
            &request_ref,
            &session_ref,
            &snapshot_ref,
            &assessment_spec_ref,
            &instrument_version_ref,
            &scoring_version_ref,
            &calibration_reference,
            norm_version,
        )
        .expect_err("direct SQL must not bypass the Rust scoring-reference boundary");
        assert_check(&error, constraint);
    }
}

#[test]
fn every_scoring_reference_rejects_unicode_numeric_whitespace_and_control_aliases() {
    let _guard = guard();
    let mut client = client();

    for (field, constraint) in [
        (
            ReferenceField::Request,
            "scoring_request_scoring_request_ref_format_check",
        ),
        (
            ReferenceField::Session,
            "scoring_request_session_ref_format_check",
        ),
        (
            ReferenceField::Snapshot,
            "scoring_request_response_snapshot_ref_format_check",
        ),
        (
            ReferenceField::AssessmentSpec,
            "scoring_request_assessment_spec_ref_format_check",
        ),
        (
            ReferenceField::InstrumentVersion,
            "scoring_request_instrument_version_ref_format_check",
        ),
        (
            ReferenceField::ScoringVersion,
            "scoring_request_scoring_version_ref_format_check",
        ),
        (
            ReferenceField::Calibration,
            "scoring_request_calibration_reference_format_check",
        ),
        (
            ReferenceField::NormVersion,
            "scoring_request_norm_version_ref_format_check",
        ),
    ] {
        assert_field_rejects_rust_invalid_aliases(&mut client, field, constraint);
    }
}

#[test]
fn migration_reapplication_replaces_a_weakened_reference_constraint() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE scoring_request \
                 DROP CONSTRAINT scoring_request_scoring_request_ref_format_check; \
             ALTER TABLE scoring_request \
                 ADD CONSTRAINT scoring_request_scoring_request_ref_format_check CHECK (\
                     scoring_request_ref = btrim(scoring_request_ref) \
                     AND scoring_request_ref <> '' \
                     AND NOT (\
                         scoring_request_ref ~ '[[:digit:]]' \
                         AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$'\
                     )\
                 );",
        )
        .unwrap();

    apply_scoring_request_migration(&mut client).unwrap();

    let error = insert_request(
        &mut client,
        "½",
        "session_upgrade_guard",
        "response_snapshot_upgrade_guard",
        "assessment_spec_upgrade_guard",
        "instrument_version_upgrade_guard",
        "scoring_version_upgrade_guard",
        "calibration_upgrade_guard",
        None,
    )
    .expect_err("migration reapplication must repair a weaker same-named reference constraint");
    assert_check(&error, "scoring_request_scoring_request_ref_format_check");
}
