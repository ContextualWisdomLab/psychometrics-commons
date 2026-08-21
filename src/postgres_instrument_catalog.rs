//! Durable startable-instrument catalog backed by product-owned `PostgreSQL` state.
//!
//! This adapter lists only releases whose persisted lifecycle currently says
//! `published`, then validates every selected row using the same stored-evidence
//! reconstruction used by session start. The returned releases are candidates
//! only; these functions neither authorize nor create sessions. Callers must use
//! the persisted session-start path for final validation because publication
//! state can change after a catalog query completes.

use crate::postgres_instrument_release::{
    published_instrument_release_snapshot_from_row, InstrumentReleaseQueryError,
    PublishedInstrumentReleaseSnapshot,
};
use crate::reference::normalized_reference;
use postgres::GenericClient;

const PUBLISHED_RELEASE_COLUMNS: &str =
    "release_ref, instrument_ref, instrument_version_ref, construct_ref, \
     item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
     calibration_reference, norm_version_ref, narrative_version_ref, \
     consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
     publication_state, created_at_unix_ms";

/// List persisted releases that are currently candidates for starting new sessions.
///
/// Rows are selected and validated in one non-locking query and ordered by
/// instrument family, exact locale, then release reference so clients can
/// present a deterministic catalog. Any malformed persisted evidence fails the
/// whole read instead of returning a partial list. This function does not
/// authorize or create sessions, choose locale fallbacks, or perform scoring.
/// The persisted session-start path must perform final locking validation.
///
/// # Errors
///
/// Returns [`InstrumentReleaseQueryError`] if `PostgreSQL` cannot execute the
/// query or if any selected row cannot be reconstructed as an immutable
/// published release.
pub fn list_startable_instrument_releases(
    client: &mut impl GenericClient,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, InstrumentReleaseQueryError> {
    let query = format!(
        "SELECT {PUBLISHED_RELEASE_COLUMNS} \
         FROM instrument_release \
         WHERE publication_state = $1 \
         ORDER BY instrument_ref, locale, release_ref"
    );
    let rows = client.query(&query, &[&"published"])?;

    rows.iter()
        .map(published_instrument_release_snapshot_from_row)
        .collect()
}

/// List currently published candidate releases for one exact instrument family.
///
/// The family filter is applied inside `PostgreSQL`, rather than by loading the
/// complete catalog and filtering in transport code. Results are validated in
/// the same non-locking query and ordered by exact locale and release reference.
/// No locale fallback is inferred. Final session creation must still reload and
/// lock the exact release because publication state may change after this read.
///
/// # Errors
///
/// Returns [`InstrumentReleaseQueryError::InvalidReference`] when
/// `instrument_ref` is blank, numeric-like, unsafe, or would change under the
/// product's canonical opaque-reference normalization. Returns another
/// [`InstrumentReleaseQueryError`] if the query fails or stored release evidence
/// cannot be reconstructed safely.
pub fn list_startable_instrument_releases_for_family(
    client: &mut impl GenericClient,
    instrument_ref: &str,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, InstrumentReleaseQueryError> {
    let canonical = normalized_reference(instrument_ref)
        .filter(|normalized| *normalized == instrument_ref)
        .ok_or(InstrumentReleaseQueryError::InvalidReference)?;
    let query = format!(
        "SELECT {PUBLISHED_RELEASE_COLUMNS} \
         FROM instrument_release \
         WHERE publication_state = $1 AND instrument_ref = $2 \
         ORDER BY locale, release_ref"
    );
    let rows = client.query(&query, &[&"published", &canonical])?;

    rows.iter()
        .map(published_instrument_release_snapshot_from_row)
        .collect()
}
