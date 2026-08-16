//! `PostgreSQL` 18 persistence for immutable instrument-release publication.
//!
//! This adapter stores product-owned publication identity and lifecycle state
//! only. Psychometric evidence remains referenced, not recomputed. The caller
//! owns the connection, credentials, and transaction boundary. Replay requires
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is visible
//! to the exact-replay classifier.

use crate::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationState,
};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const INSTRUMENT_RELEASE_MIGRATION: &str =
    include_str!("../migrations/0006_instrument_release.sql");

/// Outcome of persisting one instrument-release snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InstrumentReleasePersistenceDisposition {
    /// A new release row was inserted or an allowed state advance was applied.
    Inserted,
    /// The same immutable release identity and publication state already existed.
    Duplicate,
}

/// Fail-closed error for durable instrument-release persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum InstrumentReleasePersistenceError {
    /// A release, instrument, or evidence identity was blank or numeric-like.
    InvalidReference,
    /// Release identity was replayed with different immutable manifest evidence.
    ConflictingReplay,
    /// Stored publication state cannot legally become the incoming snapshot state.
    InvalidTransition,
    /// A timestamp cannot be represented by the bounded database column.
    InvalidTimestamp,
    /// Instrument-release persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// Durable rows cannot reconstruct the published instrument release.
    InconsistentEvidence,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for InstrumentReleasePersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "instrument release persistence references must be opaque values"
            }
            Self::ConflictingReplay => {
                "instrument release identity was replayed with conflicting evidence"
            }
            Self::InvalidTransition => {
                "instrument release publication state cannot move to an unreachable lifecycle"
            }
            Self::InvalidTimestamp => {
                "instrument release timestamp exceeds the PostgreSQL bigint range"
            }
            Self::UnsupportedIsolationLevel => {
                "instrument release persistence requires read committed isolation"
            }
            Self::InconsistentEvidence => {
                "durable instrument-release evidence cannot reconstruct the published snapshot"
            }
            Self::Database(_) => "PostgreSQL instrument-release persistence failed",
        })
    }
}

impl Error for InstrumentReleasePersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for InstrumentReleasePersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent instrument-release migration to a `PostgreSQL` connection.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_instrument_release_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(INSTRUMENT_RELEASE_MIGRATION)
}

/// Persist one immutable instrument-release manifest and its publication state.
///
/// Exact replay of the same manifest and state is idempotent. Rebinding
/// `release_ref` to a different digest, locale, item set, or other immutable
/// field fails closed. The same manifest may advance to a reachable later
/// publication state without rewriting historical identity. A snapshot that
/// would rewind or skip to an unreachable lifecycle fails closed.
///
/// # Errors
///
/// Returns [`InstrumentReleasePersistenceError`] for unsupported isolation,
/// conflicting replay, an unreachable publication-state snapshot, an invalid
/// reference or timestamp, or a database failure.
pub fn persist_instrument_release(
    transaction: &mut Transaction<'_>,
    release: &InstrumentRelease,
) -> Result<InstrumentReleasePersistenceDisposition, InstrumentReleasePersistenceError> {
    require_read_committed(transaction)?;
    let manifest = release.manifest();
    let release_ref = required_reference(manifest.release_ref())?;
    let created_at = postgres_timestamp(release.created_at_unix_ms())?;
    let item_version_refs = manifest.item_version_refs().to_vec();
    let consent_requirement_refs = manifest.consent_requirement_refs().to_vec();
    let publication_state = publication_state_name(release.state());
    let inserted = transaction.execute(
        "INSERT INTO instrument_release (\
             release_ref, instrument_ref, instrument_version_ref, construct_ref, \
             item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
             calibration_reference, norm_version_ref, narrative_version_ref, \
             consent_requirement_refs, intended_use_ref, limitations_ref, \
             content_digest, publication_state, created_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
         ON CONFLICT (release_ref) DO NOTHING",
        &[
            &release_ref,
            &manifest.instrument_ref(),
            &manifest.instrument_version_ref(),
            &manifest.construct_ref(),
            &item_version_refs,
            &manifest.locale(),
            &manifest.assessment_spec_ref(),
            &manifest.scoring_version_ref(),
            &manifest.calibration_reference(),
            &manifest.norm_version_ref(),
            &manifest.narrative_version_ref(),
            &consent_requirement_refs,
            &manifest.intended_use_ref(),
            &manifest.limitations_ref(),
            &manifest.content_digest(),
            &publication_state,
            &created_at,
        ],
    )?;
    if inserted == 1 {
        return Ok(InstrumentReleasePersistenceDisposition::Inserted);
    }

    classify_existing_release(transaction, manifest, publication_state, created_at)
}

/// Load one persisted instrument release after process restart.
///
/// Call this with the release reference a session or catalog already holds.
/// A Published reconstruction may start new sessions on that exact locale,
/// digest, and item set. Missing identity returns `None`. Duplicate stored
/// item versions or other noncanonical rows fail closed so a worker cannot
/// treat corruption as an eligible form. Exact persist replay of the loaded
/// snapshot stays [`InstrumentReleasePersistenceDisposition::Duplicate`].
/// Event history and bound publication evidence are not stored by this
/// adapter; reactivation still requires rebound approved evidence.
///
/// # Errors
///
/// Returns [`InstrumentReleasePersistenceError`] for unsupported isolation,
/// an invalid release reference, inconsistent durable evidence, or a
/// database failure.
pub fn load_instrument_release(
    transaction: &mut Transaction<'_>,
    release_ref: &str,
) -> Result<Option<InstrumentRelease>, InstrumentReleasePersistenceError> {
    require_read_committed(transaction)?;
    let release_ref = required_reference(release_ref)?;
    let header = transaction.query_opt(
        "SELECT instrument_ref, instrument_version_ref, construct_ref, item_version_refs, \
                locale, assessment_spec_ref, scoring_version_ref, calibration_reference, \
                norm_version_ref, narrative_version_ref, consent_requirement_refs, \
                intended_use_ref, limitations_ref, content_digest, publication_state, \
                created_at_unix_ms \
         FROM instrument_release WHERE release_ref = $1",
        &[&release_ref],
    )?;
    let Some(header) = header else {
        return Ok(None);
    };
    let instrument_ref: String = header.get(0);
    let instrument_version_ref: String = header.get(1);
    let construct_ref: String = header.get(2);
    let item_version_refs: Vec<String> = header.get(3);
    let locale: String = header.get(4);
    let assessment_spec_ref: String = header.get(5);
    let scoring_version_ref: String = header.get(6);
    let calibration_reference: String = header.get(7);
    let norm_version_ref: Option<String> = header.get(8);
    let narrative_version_ref: String = header.get(9);
    let consent_requirement_refs: Vec<String> = header.get(10);
    let intended_use_ref: String = header.get(11);
    let limitations_ref: String = header.get(12);
    let content_digest: String = header.get(13);
    let publication_state = publication_state_from_stored(&header.get::<_, String>(14))?;
    let created_at_unix_ms = stored_timestamp(header.get(15))?;
    let item_refs: Vec<&str> = item_version_refs.iter().map(String::as_str).collect();
    let consent_refs: Vec<&str> = consent_requirement_refs
        .iter()
        .map(String::as_str)
        .collect();
    let manifest = InstrumentReleaseManifest::new(
        release_ref,
        &instrument_ref,
        &instrument_version_ref,
        &construct_ref,
        &item_refs,
        &locale,
        &assessment_spec_ref,
        &scoring_version_ref,
        &calibration_reference,
        norm_version_ref.as_deref(),
        &narrative_version_ref,
        &consent_refs,
        &intended_use_ref,
        &limitations_ref,
        &content_digest,
    )
    .map_err(durable_evidence_error)?;
    InstrumentRelease::from_persisted_snapshot(manifest, publication_state, created_at_unix_ms)
        .map(Some)
        .map_err(durable_evidence_error)
}

fn classify_existing_release(
    transaction: &mut Transaction<'_>,
    manifest: &InstrumentReleaseManifest,
    publication_state: &str,
    created_at: i64,
) -> Result<InstrumentReleasePersistenceDisposition, InstrumentReleasePersistenceError> {
    let row = transaction.query_one(
        "SELECT instrument_ref, instrument_version_ref, construct_ref, item_version_refs, \
                locale, assessment_spec_ref, scoring_version_ref, calibration_reference, \
                norm_version_ref, narrative_version_ref, consent_requirement_refs, \
                intended_use_ref, limitations_ref, content_digest, publication_state, \
                created_at_unix_ms \
         FROM instrument_release WHERE release_ref = $1",
        &[&manifest.release_ref()],
    )?;
    let stored_identity = ReleaseIdentity {
        instrument_ref: row.get(0),
        instrument_version_ref: row.get(1),
        construct_ref: row.get(2),
        item_version_refs: row.get(3),
        locale: row.get(4),
        assessment_spec_ref: row.get(5),
        scoring_version_ref: row.get(6),
        calibration_reference: row.get(7),
        norm_version_ref: row.get(8),
        narrative_version_ref: row.get(9),
        consent_requirement_refs: row.get(10),
        intended_use_ref: row.get(11),
        limitations_ref: row.get(12),
        content_digest: row.get(13),
        created_at_unix_ms: row.get(15),
    };
    let stored_state: String = row.get(14);
    if stored_identity != ReleaseIdentity::from_manifest(manifest, created_at) {
        return Err(InstrumentReleasePersistenceError::ConflictingReplay);
    }
    if stored_state == publication_state {
        return Ok(InstrumentReleasePersistenceDisposition::Duplicate);
    }
    if !publication_state_may_replace(&stored_state, publication_state) {
        return Err(InstrumentReleasePersistenceError::InvalidTransition);
    }
    transaction.execute(
        "UPDATE instrument_release SET publication_state = $2 WHERE release_ref = $1",
        &[&manifest.release_ref(), &publication_state],
    )?;
    Ok(InstrumentReleasePersistenceDisposition::Inserted)
}

#[derive(Debug, Eq, PartialEq)]
struct ReleaseIdentity {
    instrument_ref: String,
    instrument_version_ref: String,
    construct_ref: String,
    item_version_refs: Vec<String>,
    locale: String,
    assessment_spec_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    narrative_version_ref: String,
    consent_requirement_refs: Vec<String>,
    intended_use_ref: String,
    limitations_ref: String,
    content_digest: String,
    created_at_unix_ms: i64,
}

impl ReleaseIdentity {
    fn from_manifest(manifest: &InstrumentReleaseManifest, created_at_unix_ms: i64) -> Self {
        Self {
            instrument_ref: manifest.instrument_ref().to_owned(),
            instrument_version_ref: manifest.instrument_version_ref().to_owned(),
            construct_ref: manifest.construct_ref().to_owned(),
            item_version_refs: manifest.item_version_refs().to_vec(),
            locale: manifest.locale().to_owned(),
            assessment_spec_ref: manifest.assessment_spec_ref().to_owned(),
            scoring_version_ref: manifest.scoring_version_ref().to_owned(),
            calibration_reference: manifest.calibration_reference().to_owned(),
            norm_version_ref: manifest.norm_version_ref().map(str::to_owned),
            narrative_version_ref: manifest.narrative_version_ref().to_owned(),
            consent_requirement_refs: manifest.consent_requirement_refs().to_vec(),
            intended_use_ref: manifest.intended_use_ref().to_owned(),
            limitations_ref: manifest.limitations_ref().to_owned(),
            content_digest: manifest.content_digest().to_owned(),
            created_at_unix_ms,
        }
    }
}

fn publication_state_may_replace(stored: &str, next: &str) -> bool {
    matches!(
        (stored, next),
        ("draft", "review" | "published" | "suspended" | "retired")
            | ("review", "published" | "suspended" | "retired")
            | ("published", "suspended" | "retired")
            | ("suspended", "published" | "retired")
    )
}

fn publication_state_name(state: PublicationState) -> &'static str {
    match state {
        PublicationState::Draft => "draft",
        PublicationState::Review => "review",
        PublicationState::Published => "published",
        PublicationState::Suspended => "suspended",
        PublicationState::Retired => "retired",
    }
}

fn publication_state_from_stored(
    stored: &str,
) -> Result<PublicationState, InstrumentReleasePersistenceError> {
    match stored {
        "draft" => Ok(PublicationState::Draft),
        "review" => Ok(PublicationState::Review),
        "published" => Ok(PublicationState::Published),
        "suspended" => Ok(PublicationState::Suspended),
        "retired" => Ok(PublicationState::Retired),
        _ => Err(InstrumentReleasePersistenceError::InconsistentEvidence),
    }
}

fn stored_timestamp(timestamp: i64) -> Result<u64, InstrumentReleasePersistenceError> {
    u64::try_from(timestamp).map_err(|_| InstrumentReleasePersistenceError::InconsistentEvidence)
}

fn durable_evidence_error(error: InstrumentReleaseError) -> InstrumentReleasePersistenceError {
    match error {
        InstrumentReleaseError::InvalidReference
        | InstrumentReleaseError::EmptyItemSet
        | InstrumentReleaseError::DuplicateItemReference
        | InstrumentReleaseError::InvalidLocale
        | InstrumentReleaseError::InvalidDigest
        | InstrumentReleaseError::InvalidEvidenceDigest
        | InstrumentReleaseError::InvalidEvidenceWindow
        | InstrumentReleaseError::IncompletePublicationEvidence
        | InstrumentReleaseError::PublicationEvidenceMismatch
        | InstrumentReleaseError::MissingPublicationEvidence
        | InstrumentReleaseError::PublicationEvidenceNotApproved
        | InstrumentReleaseError::PublicationEvidenceNotEffective
        | InstrumentReleaseError::InvalidTimestamp
        | InstrumentReleaseError::NonMonotonicTimestamp
        | InstrumentReleaseError::ConflictingReplay
        | InstrumentReleaseError::InvalidTransition => {
            InstrumentReleasePersistenceError::InconsistentEvidence
        }
    }
}

fn required_reference(reference: &str) -> Result<&str, InstrumentReleasePersistenceError> {
    normalized_reference(reference).ok_or(InstrumentReleasePersistenceError::InvalidReference)
}

fn postgres_timestamp(timestamp: u64) -> Result<i64, InstrumentReleasePersistenceError> {
    i64::try_from(timestamp).map_err(|_| InstrumentReleasePersistenceError::InvalidTimestamp)
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), InstrumentReleasePersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(InstrumentReleasePersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{
        durable_evidence_error, postgres_timestamp, publication_state_from_stored,
        publication_state_may_replace, required_reference, stored_timestamp,
        InstrumentReleasePersistenceError,
    };
    use crate::instrument::{InstrumentReleaseError, PublicationState};

    #[test]
    fn blank_numeric_and_overflow_inputs_fail_closed() {
        assert!(matches!(
            required_reference(" "),
            Err(InstrumentReleasePersistenceError::InvalidReference)
        ));
        assert!(matches!(
            required_reference("12"),
            Err(InstrumentReleasePersistenceError::InvalidReference)
        ));
        assert_eq!(
            required_reference("release_big_five_ko_v1").unwrap(),
            "release_big_five_ko_v1"
        );
        assert!(matches!(
            postgres_timestamp(u64::MAX),
            Err(InstrumentReleasePersistenceError::InvalidTimestamp)
        ));
        assert_eq!(postgres_timestamp(40_000).unwrap(), 40_000);
    }

    #[test]
    fn publication_state_replacement_allows_reachable_advances_only() {
        for (stored, next) in [
            ("draft", "review"),
            ("draft", "published"),
            ("draft", "suspended"),
            ("draft", "retired"),
            ("review", "published"),
            ("review", "suspended"),
            ("review", "retired"),
            ("published", "suspended"),
            ("published", "retired"),
            ("suspended", "published"),
            ("suspended", "retired"),
        ] {
            assert!(
                publication_state_may_replace(stored, next),
                "{stored} -> {next} must remain a reachable snapshot advance"
            );
        }
        for (stored, next) in [
            ("draft", "draft"),
            ("published", "draft"),
            ("published", "review"),
            ("review", "draft"),
            ("suspended", "draft"),
            ("suspended", "review"),
            ("retired", "published"),
            ("retired", "draft"),
            ("unknown", "published"),
        ] {
            assert!(
                !publication_state_may_replace(stored, next),
                "{stored} -> {next} must fail closed"
            );
        }
    }

    #[test]
    fn stored_publication_states_rebuild_or_fail_closed() {
        assert_eq!(
            publication_state_from_stored("draft").unwrap(),
            PublicationState::Draft
        );
        assert_eq!(
            publication_state_from_stored("review").unwrap(),
            PublicationState::Review
        );
        assert_eq!(
            publication_state_from_stored("published").unwrap(),
            PublicationState::Published
        );
        assert_eq!(
            publication_state_from_stored("suspended").unwrap(),
            PublicationState::Suspended
        );
        assert_eq!(
            publication_state_from_stored("retired").unwrap(),
            PublicationState::Retired
        );
        assert!(matches!(
            publication_state_from_stored("unknown"),
            Err(InstrumentReleasePersistenceError::InconsistentEvidence)
        ));
        assert_eq!(stored_timestamp(40_000).unwrap(), 40_000);
        assert!(matches!(
            stored_timestamp(-1),
            Err(InstrumentReleasePersistenceError::InconsistentEvidence)
        ));
        for error in [
            InstrumentReleaseError::InvalidReference,
            InstrumentReleaseError::EmptyItemSet,
            InstrumentReleaseError::DuplicateItemReference,
            InstrumentReleaseError::InvalidLocale,
            InstrumentReleaseError::InvalidDigest,
            InstrumentReleaseError::InvalidEvidenceDigest,
            InstrumentReleaseError::InvalidEvidenceWindow,
            InstrumentReleaseError::IncompletePublicationEvidence,
            InstrumentReleaseError::PublicationEvidenceMismatch,
            InstrumentReleaseError::MissingPublicationEvidence,
            InstrumentReleaseError::PublicationEvidenceNotApproved,
            InstrumentReleaseError::PublicationEvidenceNotEffective,
            InstrumentReleaseError::InvalidTimestamp,
            InstrumentReleaseError::NonMonotonicTimestamp,
            InstrumentReleaseError::ConflictingReplay,
            InstrumentReleaseError::InvalidTransition,
        ] {
            assert!(matches!(
                durable_evidence_error(error),
                InstrumentReleasePersistenceError::InconsistentEvidence
            ));
        }
    }
}
