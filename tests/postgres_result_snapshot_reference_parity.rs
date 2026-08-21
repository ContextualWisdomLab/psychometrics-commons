//! Immutable result persistence must match the Rust opaque-reference boundary.
//!
//! Result provenance is acquisition-critical evidence: direct SQL and migration upgrades must not
//! retain identifiers the Rust domain would reject or normalize differently.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_result_snapshot::apply_result_snapshot_migration;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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

#[derive(Debug)]
struct SnapshotRefs {
    result: String,
    participant: String,
    scoring_result: String,
    session: String,
    response: String,
    assessment: String,
    instrument: String,
    scoring: String,
    calibration: String,
    norm: Option<String>,
    narrative: String,
    supersedes: Option<String>,
}

impl SnapshotRefs {
    fn valid(suffix: &str) -> Self {
        Self {
            result: format!("result_snapshot_{suffix}"),
            participant: format!("participant_{suffix}"),
            scoring_result: format!("scoring_result_{suffix}"),
            session: format!("session_{suffix}"),
            response: format!("response_snapshot_{suffix}"),
            assessment: format!("assessment_spec_{suffix}"),
            instrument: format!("instrument_version_{suffix}"),
            scoring: format!("scoring_version_{suffix}"),
            calibration: format!("calibration_reference_{suffix}"),
            norm: Some(format!("norm_version_{suffix}")),
            narrative: format!("narrative_version_{suffix}"),
            supersedes: None,
        }
    }
}

fn insert_snapshot(
    client: &mut Client,
    refs: &SnapshotRefs,
    consent_refs: &[&str],
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
            &refs.result,
            &refs.participant,
            &refs.scoring_result,
            &refs.session,
            &refs.response,
            &refs.assessment,
            &refs.instrument,
            &refs.scoring,
            &refs.calibration,
            &refs.norm.as_deref(),
            &refs.narrative,
            &consent_refs,
            &DIGEST,
            &refs.supersedes.as_deref(),
        ],
    )
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

fn invalid_case(field: SnapshotField, invalid_ref: &str, suffix: &str) -> SnapshotRefs {
    let mut refs = SnapshotRefs::valid(suffix);
    match field {
        SnapshotField::Result => invalid_ref.clone_into(&mut refs.result),
        SnapshotField::Participant => invalid_ref.clone_into(&mut refs.participant),
        SnapshotField::ScoringResult => invalid_ref.clone_into(&mut refs.scoring_result),
        SnapshotField::Session => invalid_ref.clone_into(&mut refs.session),
        SnapshotField::Response => invalid_ref.clone_into(&mut refs.response),
        SnapshotField::Assessment => invalid_ref.clone_into(&mut refs.assessment),
        SnapshotField::Instrument => invalid_ref.clone_into(&mut refs.instrument),
        SnapshotField::Scoring => invalid_ref.clone_into(&mut refs.scoring),
        SnapshotField::Calibration => invalid_ref.clone_into(&mut refs.calibration),
        SnapshotField::Norm => refs.norm = Some(invalid_ref.to_owned()),
        SnapshotField::Narrative => invalid_ref.clone_into(&mut refs.narrative),
        SnapshotField::Supersedes => refs.supersedes = Some(invalid_ref.to_owned()),
    }
    refs
}

fn assert_field_rejects(client: &mut Client, field: SnapshotField, constraint: &str) {
    for (index, invalid_ref) in [
        "12",
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ]
    .into_iter()
    .enumerate()
    {
        let refs = invalid_case(field, invalid_ref, &format!("{}_{index}", field as u8));
        let error = insert_snapshot(client, &refs, &["consent_snapshot_service"])
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
        (
            SnapshotField::Session,
            "result_snapshot_session_ref_format_check",
        ),
        (
            SnapshotField::Response,
            "result_snapshot_response_ref_format_check",
        ),
        (
            SnapshotField::Assessment,
            "result_snapshot_spec_ref_format_check",
        ),
        (
            SnapshotField::Instrument,
            "result_snapshot_instrument_ref_format_check",
        ),
        (
            SnapshotField::Scoring,
            "result_snapshot_scoring_ref_format_check",
        ),
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
    let invalid_references = [
        "12",
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}opaque_alpha",
        "opaque_\u{0001}_alpha",
    ];

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let refs = SnapshotRefs::valid(&format!("consent_{index}"));
        let error = insert_snapshot(&mut client, &refs, &[invalid_ref])
            .expect_err("consent snapshot arrays must reject the same Rust-invalid aliases");
        assert_check(&error, "result_snapshot_consent_refs_integrity_check");
    }

    for (index, invalid_ref) in invalid_references.into_iter().enumerate() {
        let refs = SnapshotRefs::valid(&format!("construct_{index}"));
        insert_snapshot(&mut client, &refs, &["consent_snapshot_service"]).unwrap();
        let error = client
            .execute(
                "INSERT INTO result_snapshot_observation (\
                     result_snapshot_ref, observation_order, construct_ref, observation_disposition, \
                     score, standard_error\
                 ) VALUES ($1,0,$2,'scored',0.5,0.1)",
                &[&refs.result, &invalid_ref],
            )
            .expect_err("construct references must match the Rust opaque-reference boundary");
        assert_check(
            &error,
            "result_snapshot_observation_construct_ref_format_check",
        );
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
    let mut refs = SnapshotRefs::valid("upgrade_guard");
    refs.result = "½".to_owned();
    insert_snapshot(&mut client, &refs, &["consent_snapshot_service"])
        .expect("the deliberately weakened historical CHECK should admit the regression row");

    let error = apply_result_snapshot_migration(&mut client)
        .expect_err("migration reapplication must reject historical Rust-invalid result identity");
    assert_check(&error, "result_snapshot_ref_format_check");

    let constraint = client
        .query_one(
            "SELECT pg_get_constraintdef(oid), convalidated \
             FROM pg_constraint \
             WHERE conrelid = 'result_snapshot'::regclass \
               AND conname = 'result_snapshot_ref_format_check'",
            &[],
        )
        .expect("failed migration reapplication must preserve the preexisting CHECK constraint");
    let definition: String = constraint.get(0);
    let validated: bool = constraint.get(1);
    assert!(
        definition.contains("btrim(result_snapshot_ref)"),
        "failed reapplication must roll back DROP/ADD and preserve the previous CHECK definition"
    );
    assert!(
        validated,
        "failed reapplication must leave the previous CHECK constraint validated"
    );
}
