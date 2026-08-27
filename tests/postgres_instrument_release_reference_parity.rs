//! `PostgreSQL` instrument-release identity must match the Rust immutable-manifest boundary.
//!
//! The Rust domain trims Unicode outer whitespace and rejects embedded controls,
//! default-ignorable characters, and numeric-like opaque identifiers under `char::is_numeric`.
//! Instrument item and consent arrays also require canonical, unique opaque references. Direct SQL
//! and migration reapplication must not leave durable publication provenance Rust cannot construct.

use postgres::{error::SqlState, Client, NoTls};
use psychometrics_commons_runtime::postgres_instrument_release::apply_instrument_release_migration;
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
            "DROP SCHEMA IF EXISTS instrument_release_reference_parity_test CASCADE; \
             CREATE SCHEMA instrument_release_reference_parity_test; \
             SET search_path TO instrument_release_reference_parity_test;",
        )
        .unwrap();
    apply_instrument_release_migration(&mut client).unwrap();
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
fn insert_release(
    client: &mut Client,
    release_ref: &str,
    instrument_ref: &str,
    instrument_version_ref: &str,
    construct_ref: &str,
    item_version_refs: &[String],
    assessment_spec_ref: &str,
    scoring_version_ref: &str,
    calibration_reference: &str,
    norm_version_ref: Option<&str>,
    narrative_version_ref: &str,
    consent_requirement_refs: &[String],
    intended_use_ref: &str,
    limitations_ref: &str,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO instrument_release (\
             release_ref, instrument_ref, instrument_version_ref, construct_ref, \
             item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
             calibration_reference, norm_version_ref, narrative_version_ref, \
             consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
             publication_state, created_at_unix_ms\
         ) VALUES ($1,$2,$3,$4,$5,'ko-KR',$6,$7,$8,$9,$10,$11,$12,$13,$14,'draft',40000)",
        &[
            &release_ref,
            &instrument_ref,
            &instrument_version_ref,
            &construct_ref,
            &item_version_refs,
            &assessment_spec_ref,
            &scoring_version_ref,
            &calibration_reference,
            &norm_version_ref,
            &narrative_version_ref,
            &consent_requirement_refs,
            &intended_use_ref,
            &limitations_ref,
            &RELEASE_DIGEST,
        ],
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReferenceField {
    Release,
    Instrument,
    InstrumentVersion,
    Construct,
    AssessmentSpec,
    ScoringVersion,
    Calibration,
    NormVersion,
    NarrativeVersion,
    IntendedUse,
    Limitations,
}

fn assert_scalar_field_rejects_rust_invalid_aliases(
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
        let mut release_ref = format!("release_{suffix}");
        let mut instrument_ref = format!("instrument_{suffix}");
        let mut instrument_version_ref = format!("instrument_version_{suffix}");
        let mut construct_ref = format!("construct_{suffix}");
        let item_refs = vec![format!("item_version_{suffix}_alpha")];
        let mut assessment_spec_ref = format!("assessment_spec_{suffix}");
        let mut scoring_version_ref = format!("scoring_version_{suffix}");
        let mut calibration_reference = format!("calibration_reference_{suffix}");
        let norm_version_ref = format!("norm_version_{suffix}");
        let mut narrative_version_ref = format!("narrative_version_{suffix}");
        let consent_refs = vec![format!("consent_requirement_{suffix}")];
        let mut intended_use_ref = format!("intended_use_{suffix}");
        let mut limitations_ref = format!("limitations_{suffix}");

        match field {
            ReferenceField::Release => invalid_ref.clone_into(&mut release_ref),
            ReferenceField::Instrument => invalid_ref.clone_into(&mut instrument_ref),
            ReferenceField::InstrumentVersion => {
                invalid_ref.clone_into(&mut instrument_version_ref);
            }
            ReferenceField::Construct => invalid_ref.clone_into(&mut construct_ref),
            ReferenceField::AssessmentSpec => invalid_ref.clone_into(&mut assessment_spec_ref),
            ReferenceField::ScoringVersion => invalid_ref.clone_into(&mut scoring_version_ref),
            ReferenceField::Calibration => invalid_ref.clone_into(&mut calibration_reference),
            ReferenceField::NormVersion => {}
            ReferenceField::NarrativeVersion => invalid_ref.clone_into(&mut narrative_version_ref),
            ReferenceField::IntendedUse => invalid_ref.clone_into(&mut intended_use_ref),
            ReferenceField::Limitations => invalid_ref.clone_into(&mut limitations_ref),
        }
        let norm_ref = if field == ReferenceField::NormVersion {
            Some(invalid_ref)
        } else {
            Some(norm_version_ref.as_str())
        };

        let error = insert_release(
            client,
            &release_ref,
            &instrument_ref,
            &instrument_version_ref,
            &construct_ref,
            &item_refs,
            &assessment_spec_ref,
            &scoring_version_ref,
            &calibration_reference,
            norm_ref,
            &narrative_version_ref,
            &consent_refs,
            &intended_use_ref,
            &limitations_ref,
        )
        .expect_err("direct SQL must not bypass the Rust instrument-reference boundary");
        assert_check(&error, constraint);
    }
}

#[test]
fn every_scalar_release_reference_rejects_rust_invalid_aliases() {
    let _guard = guard();
    let mut client = client();

    for (field, constraint) in [
        (
            ReferenceField::Release,
            "instrument_release_release_ref_format_check",
        ),
        (
            ReferenceField::Instrument,
            "instrument_release_instrument_ref_format_check",
        ),
        (
            ReferenceField::InstrumentVersion,
            "instrument_release_version_ref_format_check",
        ),
        (
            ReferenceField::Construct,
            "instrument_release_construct_ref_format_check",
        ),
        (
            ReferenceField::AssessmentSpec,
            "instrument_release_spec_ref_format_check",
        ),
        (
            ReferenceField::ScoringVersion,
            "instrument_release_scoring_ref_format_check",
        ),
        (
            ReferenceField::Calibration,
            "instrument_release_calibration_ref_format_check",
        ),
        (
            ReferenceField::NormVersion,
            "instrument_release_norm_ref_format_check",
        ),
        (
            ReferenceField::NarrativeVersion,
            "instrument_release_narrative_ref_format_check",
        ),
        (
            ReferenceField::IntendedUse,
            "instrument_release_intended_use_ref_format_check",
        ),
        (
            ReferenceField::Limitations,
            "instrument_release_limitations_ref_format_check",
        ),
    ] {
        assert_scalar_field_rejects_rust_invalid_aliases(&mut client, field, constraint);
    }
}

fn assert_item_reference_array_rejects_rust_invalid_aliases(client: &mut Client) {
    for invalid_ref in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}item_alpha",
        "item_\u{0001}_alpha",
        "item_\u{00ad}_alpha",
        "item_\u{200b}_alpha",
        "item_\u{200d}_alpha",
        "item_\u{2060}_alpha",
        "item_\u{fe0f}_alpha",
        "item_\u{feff}_alpha",
        "item_\u{e0001}_alpha",
    ] {
        let item_refs = vec![invalid_ref.to_owned()];
        let consent_refs = vec!["consent_service_v1".to_owned()];
        let error = insert_release(
            client,
            &format!("release_item_{}", invalid_ref.len()),
            "instrument_big_five",
            "instrument_version_big_five_v1",
            "construct_big_five",
            &item_refs,
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_v1",
            None,
            "narrative_version_big_five_v1",
            &consent_refs,
            "intended_use_self_reflection_v1",
            "limitations_nonclinical_v1",
        )
        .expect_err("item-version arrays must enforce the Rust reference boundary");
        assert_check(&error, "instrument_release_item_refs_format_check");
    }
}

fn assert_consent_reference_array_rejects_rust_invalid_aliases(client: &mut Client) {
    for invalid_ref in [
        "½",
        "²",
        "Ⅳ",
        "\u{00a0}consent_alpha",
        "consent_\u{0001}_alpha",
        "consent_\u{00ad}_alpha",
        "consent_\u{200b}_alpha",
        "consent_\u{200d}_alpha",
        "consent_\u{2060}_alpha",
        "consent_\u{fe0f}_alpha",
        "consent_\u{feff}_alpha",
        "consent_\u{e0001}_alpha",
    ] {
        let item_refs = vec!["item_version_alpha".to_owned()];
        let consent_refs = vec![invalid_ref.to_owned()];
        let error = insert_release(
            client,
            &format!("release_consent_{}", invalid_ref.len()),
            "instrument_big_five",
            "instrument_version_big_five_v1",
            "construct_big_five",
            &item_refs,
            "assessment_spec_big_five_v1",
            "scoring_version_big_five_v1",
            "calibration_big_five_v1",
            None,
            "narrative_version_big_five_v1",
            &consent_refs,
            "intended_use_self_reflection_v1",
            "limitations_nonclinical_v1",
        )
        .expect_err("consent arrays must enforce the Rust reference boundary");
        assert_check(&error, "instrument_release_consent_refs_format_check");
    }
}

#[test]
fn item_and_consent_reference_arrays_require_canonical_unique_opaque_values() {
    let _guard = guard();
    let mut client = client();

    assert_item_reference_array_rejects_rust_invalid_aliases(&mut client);
    assert_consent_reference_array_rejects_rust_invalid_aliases(&mut client);

    let duplicate_items = vec![
        "item_version_alpha".to_owned(),
        "item_version_alpha".to_owned(),
    ];
    let consent_refs = vec!["consent_service_v1".to_owned()];
    let error = insert_release(
        &mut client,
        "release_duplicate_items",
        "instrument_big_five",
        "instrument_version_big_five_v1",
        "construct_big_five",
        &duplicate_items,
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        None,
        "narrative_version_big_five_v1",
        &consent_refs,
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
    )
    .expect_err("duplicate item references must not be durably representable");
    assert_check(&error, "instrument_release_item_refs_format_check");

    let item_refs = vec!["item_version_alpha".to_owned()];
    let duplicate_consents = vec![
        "consent_service_v1".to_owned(),
        "consent_service_v1".to_owned(),
    ];
    let error = insert_release(
        &mut client,
        "release_duplicate_consents",
        "instrument_big_five",
        "instrument_version_big_five_v1",
        "construct_big_five",
        &item_refs,
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_v1",
        None,
        "narrative_version_big_five_v1",
        &duplicate_consents,
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
    )
    .expect_err("duplicate consent references must not be durably representable");
    assert_check(&error, "instrument_release_consent_refs_format_check");
}

#[test]
fn migration_reapplication_replaces_weakened_reference_constraints() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE instrument_release \
                 DROP CONSTRAINT instrument_release_release_ref_format_check; \
             ALTER TABLE instrument_release \
                 ADD CONSTRAINT instrument_release_release_ref_format_check CHECK (\
                     release_ref = btrim(release_ref) \
                     AND release_ref <> '' \
                     AND NOT (\
                         release_ref ~ '[[:digit:]]' \
                         AND release_ref ~ '^[[:digit:]+,.eE-]+$'\
                     )\
                 );",
        )
        .unwrap();

    apply_instrument_release_migration(&mut client).unwrap();

    let item_refs = vec!["item_version_upgrade_guard".to_owned()];
    let consent_refs = vec!["consent_service_upgrade_guard".to_owned()];
    let error = insert_release(
        &mut client,
        "release_\u{200b}_upgrade_guard",
        "instrument_upgrade_guard",
        "instrument_version_upgrade_guard",
        "construct_upgrade_guard",
        &item_refs,
        "assessment_spec_upgrade_guard",
        "scoring_version_upgrade_guard",
        "calibration_upgrade_guard",
        None,
        "narrative_version_upgrade_guard",
        &consent_refs,
        "intended_use_upgrade_guard",
        "limitations_upgrade_guard",
    )
    .expect_err("migration reapplication must restore the stronger reference predicate");
    assert_check(&error, "instrument_release_release_ref_format_check");
}

#[test]
fn migration_reapplication_fails_closed_on_historical_invalid_identity() {
    let _guard = guard();
    let mut client = client();

    client
        .batch_execute(
            "ALTER TABLE instrument_release \
                 DROP CONSTRAINT instrument_release_release_ref_format_check; \
             ALTER TABLE instrument_release \
                 ADD CONSTRAINT instrument_release_release_ref_format_check CHECK (\
                     release_ref = btrim(release_ref) AND release_ref <> ''\
                 );",
        )
        .unwrap();

    let item_refs = vec!["item_version_historical".to_owned()];
    let consent_refs = vec!["consent_service_historical".to_owned()];
    insert_release(
        &mut client,
        "release_\u{200b}_historical",
        "instrument_historical",
        "instrument_version_historical",
        "construct_historical",
        &item_refs,
        "assessment_spec_historical",
        "scoring_version_historical",
        "calibration_historical",
        None,
        "narrative_version_historical",
        &consent_refs,
        "intended_use_historical",
        "limitations_historical",
    )
    .expect("weakened historical predicate must admit the regression fixture");

    let error = apply_instrument_release_migration(&mut client)
        .expect_err("upgrade must fail closed instead of blessing an invalid historical identity");
    assert_check(&error, "instrument_release_release_ref_format_check");
}
