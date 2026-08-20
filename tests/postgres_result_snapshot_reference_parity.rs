//! Immutable result persistence must match the Rust opaque-reference boundary.
//!
//! Result provenance is acquisition-critical evidence: direct SQL and migration upgrades must not
//! retain identifiers the Rust domain would reject or normalize differently.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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
            "DROP SCHEMA IF EXISTS result_snapshot_reference_parity_test CASCADE; \
             CREATE SCHEMA result_snapshot_reference_parity_test; \
             SET search_path TO result_snapshot_reference_parity_test;",
        )
        .unwrap();
    apply_result_snapshot_migration(&mut client).unwrap();
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
fn insert_snapshot(
    client: &mut Client,
    result_ref: &str,
    participant_ref: &str,
    scoring_result_ref: &str,
    session_ref: &str,
    response_ref: &str,
    assessment_ref: &str,
    instrument_ref: &str,
    scoring_ref: &str,
    calibration_ref: &str,
    norm_ref: Option<&str>,
    narrative_ref: &str,
    consent_refs: Vec<&str>,
    supersedes_ref: Option<&str>,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO result_snapshot (\
             result_snapshot_ref, participant_ref, scoring_result_ref, session_ref, \
             response_snapshot_ref, assessment_spec_ref, instrument_version_ref, \
             scoring_version_ref, calibration_reference, norm_version_ref, \
             requested_output_schema_version, narrative_version_ref, consent_snapshot_refs, \
             engine_artifact_digest, created_at_unix_ms, supersedes_ref\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,1,$11,$12,$13,70000,$14)",
        &[
            &result_ref,
            &participant_ref,
            &scoring_result_ref,
            &session_ref,
            &response_ref,
            &assessment_ref,
            &instrument_ref,
            &scoring_ref,
            &calibration_ref,
            &norm_ref,
            &narrative_ref,
            &consent_refs,
            &DIGEST,
            &supersedes_ref,
        ],
    )
}

fn valid_values(suffix: &str) -> [String; 11] {
    [
        format!("result_snapshot_{suffix}"),
        format!("participant_{suffix}"),
        format!("scoring_result_{suffix}"),
        format!("session_{suffix}"),
        format!("response_snapshot_{suffix}"),
        format!("assessment_spec_{suffix}"),
        format!("instrument_version_{suffix}"),
        format!("scoring_version_{suffix}"),
        format!("calibration_reference_{suffix}"),
        format!("norm_version_{suffix}"),
        format!("narrative_version_{suffix}"),
    ]
}

#[derive(Clone, Copy)]
enum SnapshotField {
    Result,
    Participant,
    ScoringResult,
    Session,
    Response,
    Assessment,
    Instrument,
    Scoring,
    Calibration,
    Norm,
    Narrative,
    Supersedes,
}

fn assert_field_rejects(client: &mut Client, field: SnapshotField, constraint: &str) {
    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let mut values = valid_values(&format!("{}_{index}", field as u8));
        let mut norm = Some(values[9].as_str());
        let mut supersedes = None;
        match field {
            SnapshotField::Result => values[0] = invalid_ref.to_owned(),
            SnapshotField::Participant => values[1] = invalid_ref.to_owned(),
            SnapshotField::ScoringResult => values[2] = invalid_ref.to_owned(),
            SnapshotField::Session => values[3] = invalid_ref.to_owned(),
            SnapshotField::Response => values[4] = invalid_ref.to_owned(),
            SnapshotField::Assessment => values[5] = invalid_ref.to_owned(),
            SnapshotField::Instrument => values[6] = invalid_ref.to_owned(),
            SnapshotField::Scoring => values[7] = invalid_ref.to_owned(),
            SnapshotField::Calibration => values[8] = invalid_ref.to_owned(),
            SnapshotField::Norm => norm = Some(invalid_ref),
            SnapshotField::Narrative => values[10] = invalid_ref.to_owned(),
            SnapshotField::Supersedes => supersedes = Some(invalid_ref),
        }

        let error = insert_snapshot(
            client,
            &values[0],
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            &values[6],
            &values[7],
            &values[8],
            norm,
            &values[10],
            vec!["consent_snapshot_service"],
            supersedes,
        )
        .expect_err("every durable result reference must match the Rust boundary");
        assert_check(&error, constraint);
    }
}

#[test]
fn every_snapshot_reference_column_rejects_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();

    for (field, constraint) in [
        (SnapshotField::Result, "result_snapshot_ref_format_check"),
        (
            SnapshotField::Participant,
            "result_snapshot_participant_ref_format_check",
        ),
        (
            SnapshotField::ScoringResult,
            "result_snapshot_scoring_result_ref_format_check",
        ),
        (SnapshotField::Session, "result_snapshot_session_ref_format_check"),
        (SnapshotField::Response, "result_snapshot_response_ref_format_check"),
        (SnapshotField::Assessment, "result_snapshot_spec_ref_format_check"),
        (
            SnapshotField::Instrument,
            "result_snapshot_instrument_ref_format_check",
        ),
        (SnapshotField::Scoring, "result_snapshot_scoring_ref_format_check"),
        (
            SnapshotField::Calibration,
            "result_snapshot_calibration_ref_format_check",
        ),
        (SnapshotField::Norm, "result_snapshot_norm_ref_format_check"),
        (
            SnapshotField::Narrative,
            "result_snapshot_narrative_ref_format_check",
        ),
        (
            SnapshotField::Supersedes,
            "result_snapshot_supersedes_ref_format_check",
        ),
    ] {
        assert_field_rejects(&mut client, field, constraint);
    }
}

#[test]
fn consent_array_and_observation_construct_share_the_rust_reference_boundary() {
    let _guard = guard();
    let mut client = client();

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let values = valid_values(&format!("consent_{index}"));
        let error = insert_snapshot(
            &mut client,
            &values[0],
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            &values[6],
            &values[7],
            &values[8],
            Some(&values[9]),
            &values[10],
            vec![invalid_ref],
            None,
        )
        .expect_err("consent snapshot arrays must reject the same Rust-invalid aliases");
        assert_check(&error, "result_snapshot_consent_refs_integrity_check");
    }

    for (index, invalid_ref) in ["½", "²", "Ⅳ", "\u{00a0}opaque_alpha", "opaque_\u{0001}_alpha"]
        .into_iter()
        .enumerate()
    {
        let values = valid_values(&format!("construct_{index}"));
        insert_snapshot(
            &mut client,
            &values[0],
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            &values[6],
            &values[7],
            &values[8],
            Some(&values[9]),
            &values[10],
            vec!["consent_snapshot_service"],
            None,
        )
        .unwrap();
        let error = client
            .execute(
                "INSERT INTO result_snapshot_observation (\
                     result_snapshot_ref, observation_order, construct_ref, observation_disposition, \
                     score, standard_error\
                 ) VALUES ($1,0,$2,'scored',0.5,0.1)",
                &[&values[0], &invalid_ref],
            )
            .expect_err("construct references must match the Rust opaque-reference boundary");
        assert_check(&error, "result_snapshot_observation_construct_ref_format_check");
    }
}

#[test]
fn migration_reapplication_revalidates_existing_result_rows() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE result_snapshot DROP CONSTRAINT result_snapshot_ref_format_check; \
             ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_ref_format_check CHECK (\
                 result_snapshot_ref = btrim(result_snapshot_ref) AND result_snapshot_ref <> ''\
             );",
        )
        .unwrap();
    let values = valid_values("upgrade_guard");
    insert_snapshot(
        &mut client,
        "½",
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
        &values[7],
        &values[8],
        Some(&values[9]),
        &values[10],
        vec!["consent_snapshot_service"],
        None,
    )
    .expect("the deliberately weakened historical CHECK should admit the regression row");

    let error = apply_result_snapshot_migration(&mut client)
        .expect_err("migration reapplication must reject historical Rust-invalid result identity");
    assert_check(&error, "result_snapshot_ref_format_check");
}
