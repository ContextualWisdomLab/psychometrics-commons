//! `PostgreSQL` 18 persistence for immutable instrument-release publication.
//!
//! This adapter stores product-owned publication identity and lifecycle state
//! only. Psychometric evidence remains referenced, not recomputed. The caller
//! owns the connection, credentials, and transaction boundary. Replay requires
//! `READ COMMITTED` so a concurrent insert that wins a unique-key race is visible
//! to the exact-replay classifier.

use crate::instrument::{InstrumentRelease, InstrumentReleaseManifest, PublicationState};
use crate::reference::normalized_reference;
use postgres::{GenericClient, Row, Transaction};
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

/// Immutable, database-validated release evidence that may start a new assessment session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedInstrumentReleaseSnapshot {
    manifest: InstrumentReleaseManifest,
    created_at_unix_ms: u64,
}

impl PublishedInstrumentReleaseSnapshot {
    /// Return the exact immutable manifest loaded from the operational store.
    #[must_use]
    pub const fn manifest(&self) -> &InstrumentReleaseManifest {
        &self.manifest
    }

    /// Return the server-authoritative creation time stored with this release.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Wrap a manifest that a published-release load already accepted.
    ///
    /// HTTP start and tests use this after
    /// [`load_published_instrument_release`] or an equivalent store proof.
    /// It does not mark a draft manifest as published.
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentReleaseQueryError::InvalidStoredValue`] when the stored
    /// creation time is zero.
    pub fn from_published_manifest(
        manifest: InstrumentReleaseManifest,
        created_at_unix_ms: u64,
    ) -> Result<Self, InstrumentReleaseQueryError> {
        if created_at_unix_ms == 0 {
            return Err(InstrumentReleaseQueryError::InvalidStoredValue);
        }
        Ok(Self {
            manifest,
            created_at_unix_ms,
        })
    }
}

/// Fail-closed error for loading a release that is eligible to start a new session.
#[derive(Debug)]
#[non_exhaustive]
pub enum InstrumentReleaseQueryError {
    /// The requested release identity is blank, numeric-like, or not already canonical.
    InvalidReference,
    /// The requested locale is not an exact BCP 47-style locale tag.
    InvalidLocale,
    /// No persisted release exists for the requested release identity.
    NotFound,
    /// The persisted release exists but its exact locale differs from the requested locale.
    LocaleMismatch,
    /// The persisted release is not currently in the `Published` lifecycle state.
    NotPublished,
    /// Persisted release evidence violates the immutable domain contract.
    InvalidStoredValue,
    /// `PostgreSQL` rejected or could not execute the query.
    Database(postgres::Error),
}

impl Display for InstrumentReleaseQueryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "instrument release query requires an exact opaque release reference"
            }
            Self::InvalidLocale => "instrument release query requires an exact BCP 47-style locale",
            Self::NotFound => "requested instrument release does not exist",
            Self::LocaleMismatch => "requested locale does not match the persisted release locale",
            Self::NotPublished => "requested instrument release cannot start new sessions",
            Self::InvalidStoredValue => {
                "persisted instrument release violates the immutable release contract"
            }
            Self::Database(_) => "PostgreSQL instrument-release query failed",
        })
    }
}

impl Error for InstrumentReleaseQueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidReference
            | Self::InvalidLocale
            | Self::NotFound
            | Self::LocaleMismatch
            | Self::NotPublished
            | Self::InvalidStoredValue => None,
        }
    }
}

impl From<postgres::Error> for InstrumentReleaseQueryError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

struct StoredInstrumentReleaseRow {
    release_ref: String,
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
    publication_state: String,
    created_at_unix_ms: i64,
}

impl StoredInstrumentReleaseRow {
    fn from_row(row: &Row) -> Self {
        Self {
            release_ref: row.get("release_ref"),
            instrument_ref: row.get("instrument_ref"),
            instrument_version_ref: row.get("instrument_version_ref"),
            construct_ref: row.get("construct_ref"),
            item_version_refs: row.get("item_version_refs"),
            locale: row.get("locale"),
            assessment_spec_ref: row.get("assessment_spec_ref"),
            scoring_version_ref: row.get("scoring_version_ref"),
            calibration_reference: row.get("calibration_reference"),
            norm_version_ref: row.get("norm_version_ref"),
            narrative_version_ref: row.get("narrative_version_ref"),
            consent_requirement_refs: row.get("consent_requirement_refs"),
            intended_use_ref: row.get("intended_use_ref"),
            limitations_ref: row.get("limitations_ref"),
            content_digest: row.get("content_digest"),
            publication_state: row.get("publication_state"),
            created_at_unix_ms: row.get("created_at_unix_ms"),
        }
    }

    fn into_published_snapshot(
        self,
        requested_locale: &str,
    ) -> Result<PublishedInstrumentReleaseSnapshot, InstrumentReleaseQueryError> {
        if self.publication_state != "published" {
            return Err(InstrumentReleaseQueryError::NotPublished);
        }
        if self.locale != requested_locale {
            return Err(InstrumentReleaseQueryError::LocaleMismatch);
        }
        let created_at_unix_ms = u64::try_from(self.created_at_unix_ms)
            .ok()
            .filter(|timestamp| *timestamp > 0)
            .ok_or(InstrumentReleaseQueryError::InvalidStoredValue)?;
        let item_refs = self
            .item_version_refs
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let consent_refs = self
            .consent_requirement_refs
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let manifest = InstrumentReleaseManifest::new(
            &self.release_ref,
            &self.instrument_ref,
            &self.instrument_version_ref,
            &self.construct_ref,
            &item_refs,
            &self.locale,
            &self.assessment_spec_ref,
            &self.scoring_version_ref,
            &self.calibration_reference,
            self.norm_version_ref.as_deref(),
            &self.narrative_version_ref,
            &consent_refs,
            &self.intended_use_ref,
            &self.limitations_ref,
            &self.content_digest,
        )
        .map_err(|_| InstrumentReleaseQueryError::InvalidStoredValue)?;

        if (
            manifest.item_version_refs(),
            manifest.consent_requirement_refs(),
        ) != (
            self.item_version_refs.as_slice(),
            self.consent_requirement_refs.as_slice(),
        ) {
            return Err(InstrumentReleaseQueryError::InvalidStoredValue);
        }
        let manifest_core_identity = (
            manifest.release_ref(),
            manifest.instrument_ref(),
            manifest.instrument_version_ref(),
            manifest.construct_ref(),
            manifest.locale(),
            manifest.assessment_spec_ref(),
            manifest.scoring_version_ref(),
            manifest.calibration_reference(),
        );
        let stored_core_identity = (
            self.release_ref.as_str(),
            self.instrument_ref.as_str(),
            self.instrument_version_ref.as_str(),
            self.construct_ref.as_str(),
            self.locale.as_str(),
            self.assessment_spec_ref.as_str(),
            self.scoring_version_ref.as_str(),
            self.calibration_reference.as_str(),
        );
        let manifest_presentation_identity = (
            manifest.norm_version_ref(),
            manifest.narrative_version_ref(),
            manifest.intended_use_ref(),
            manifest.limitations_ref(),
            manifest.content_digest(),
        );
        let stored_presentation_identity = (
            self.norm_version_ref.as_deref(),
            self.narrative_version_ref.as_str(),
            self.intended_use_ref.as_str(),
            self.limitations_ref.as_str(),
            self.content_digest.as_str(),
        );
        if manifest_core_identity != stored_core_identity
            || manifest_presentation_identity != stored_presentation_identity
        {
            return Err(InstrumentReleaseQueryError::InvalidStoredValue);
        }

        Ok(PublishedInstrumentReleaseSnapshot {
            manifest,
            created_at_unix_ms,
        })
    }
}

pub(crate) fn published_instrument_release_snapshot_from_row(
    row: &Row,
) -> Result<PublishedInstrumentReleaseSnapshot, InstrumentReleaseQueryError> {
    let stored = StoredInstrumentReleaseRow::from_row(row);
    let locale = stored.locale.clone();
    stored.into_published_snapshot(&locale)
}

/// Load one exact published release and lock it for session start.
///
/// The caller must provide the canonical release reference and the exact requested
/// locale. The row is locked with `SELECT … FOR UPDATE` so a concurrent persist
/// Suspend or Retire cannot hide from this transaction. A release in Draft,
/// Review, Suspended, or Retired state is never returned as session-eligible.
/// Stored columns are reconstructed through [`InstrumentReleaseManifest::new`]
/// before they leave the persistence boundary. This boundary requires the
/// caller's locale spelling to already be canonical. It does not select a
/// fallback locale and does not perform psychometric scoring.
///
/// # Errors
///
/// Returns [`InstrumentReleaseQueryError`] when the caller supplies a malformed
/// identity or locale, the release is missing, the exact locale does not match,
/// the release is not Published, stored evidence violates the immutable manifest
/// contract, or the database query fails.
pub fn load_published_instrument_release(
    client: &mut impl GenericClient,
    release_ref: &str,
    locale: &str,
) -> Result<PublishedInstrumentReleaseSnapshot, InstrumentReleaseQueryError> {
    let canonical_release_ref =
        normalized_reference(release_ref).ok_or(InstrumentReleaseQueryError::InvalidReference)?;
    if canonical_release_ref != release_ref {
        return Err(InstrumentReleaseQueryError::InvalidReference);
    }
    if !valid_exact_locale(locale) {
        return Err(InstrumentReleaseQueryError::InvalidLocale);
    }

    let row = client
        .query_opt(
            "SELECT release_ref, instrument_ref, instrument_version_ref, construct_ref, \
                    item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
                    calibration_reference, norm_version_ref, narrative_version_ref, \
                    consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
                    publication_state, created_at_unix_ms \
             FROM instrument_release WHERE release_ref = $1 \
             FOR UPDATE",
            &[&canonical_release_ref],
        )?
        .ok_or(InstrumentReleaseQueryError::NotFound)?;
    StoredInstrumentReleaseRow::from_row(&row).into_published_snapshot(locale)
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

fn required_reference(reference: &str) -> Result<&str, InstrumentReleasePersistenceError> {
    normalized_reference(reference).ok_or(InstrumentReleasePersistenceError::InvalidReference)
}

fn valid_exact_locale(locale: &str) -> bool {
    if locale != locale.trim() {
        return false;
    }
    let mut subtags = locale.split('-');
    let primary = subtags.next().unwrap_or_default();
    if !(2..=8).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    subtags.all(|subtag| {
        (1..=8).contains(&subtag.len()) && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
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
        postgres_timestamp, publication_state_may_replace, required_reference, valid_exact_locale,
        InstrumentReleasePersistenceError, InstrumentReleaseQueryError,
        PublishedInstrumentReleaseSnapshot,
    };

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
    fn published_snapshot_and_query_errors_tell_the_caller_what_to_do_next() {
        assert!(matches!(
            PublishedInstrumentReleaseSnapshot::from_published_manifest(
                crate::instrument::InstrumentReleaseManifest::new(
                    "release_big_five_ko_v1",
                    "instrument_big_five",
                    "instrument_version_big_five_ko_v1",
                    "construct_big_five",
                    &["item_version_001"],
                    "ko-KR",
                    "assessment_spec_big_five_v1",
                    "scoring_version_big_five_v1",
                    "calibration_big_five_ko_v1",
                    None,
                    "narrative_version_big_five_v1",
                    &["consent_service_v1"],
                    "intended_use_self_reflection_v1",
                    "limitations_nonclinical_v1",
                    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                )
                .unwrap(),
                0,
            ),
            Err(InstrumentReleaseQueryError::InvalidStoredValue)
        ));
        assert!(valid_exact_locale("ko-KR"));
        assert!(!valid_exact_locale(" ko-KR"));
        for (error, expected) in [
            (
                InstrumentReleaseQueryError::InvalidReference,
                "instrument release query requires an exact opaque release reference",
            ),
            (
                InstrumentReleaseQueryError::InvalidLocale,
                "instrument release query requires an exact BCP 47-style locale",
            ),
            (
                InstrumentReleaseQueryError::NotFound,
                "requested instrument release does not exist",
            ),
            (
                InstrumentReleaseQueryError::LocaleMismatch,
                "requested locale does not match the persisted release locale",
            ),
            (
                InstrumentReleaseQueryError::NotPublished,
                "requested instrument release cannot start new sessions",
            ),
            (
                InstrumentReleaseQueryError::InvalidStoredValue,
                "persisted instrument release violates the immutable release contract",
            ),
        ] {
            assert_eq!(error.to_string(), expected);
            assert!(std::error::Error::source(&error).is_none());
        }
        let source = postgres::Config::new()
            .host("/no/such/psychometrics-commons.socket")
            .port(1)
            .user("postgres")
            .dbname("psychometrics_commons_test")
            .connect_timeout(std::time::Duration::from_millis(50))
            .connect(postgres::NoTls)
            .map(|_| ())
            .expect_err("missing local socket must fail closed");
        let query_error = InstrumentReleaseQueryError::from(source);
        assert_eq!(
            query_error.to_string(),
            "PostgreSQL instrument-release query failed"
        );
        assert!(std::error::Error::source(&query_error).is_some());
    }
}
