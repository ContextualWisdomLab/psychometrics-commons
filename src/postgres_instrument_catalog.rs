//! Durable startable-instrument catalog backed by product-owned `PostgreSQL` state.
//!
//! This adapter lists only releases whose persisted lifecycle currently says
//! `published`, then validates every selected row using the same stored-evidence
//! reconstruction used by session start. The returned releases are candidates
//! only; this function neither authorizes nor creates sessions. Callers must use
//! the persisted session-start path for final validation because publication
//! state can change after the catalog query completes.

use crate::postgres_instrument_release::{
    published_instrument_release_snapshot_from_row, InstrumentReleaseQueryError,
    PublishedInstrumentReleaseSnapshot,
};
use postgres::GenericClient;

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
    let rows = client.query(
        "SELECT release_ref, instrument_ref, instrument_version_ref, construct_ref, \
                item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
                calibration_reference, norm_version_ref, narrative_version_ref, \
                consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
                publication_state, created_at_unix_ms \
         FROM instrument_release \
         WHERE publication_state = $1 \
         ORDER BY instrument_ref, locale, release_ref",
        &[&"published"],
    )?;

    rows.iter()
        .map(published_instrument_release_snapshot_from_row)
        .collect()
}
