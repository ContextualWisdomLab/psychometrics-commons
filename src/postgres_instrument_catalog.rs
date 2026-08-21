//! Durable startable-instrument catalog backed by product-owned `PostgreSQL` state.
//!
//! This adapter lists only releases whose persisted lifecycle currently says
//! `published`, then revalidates each exact release through the same sealed
//! loader used by session start. The list is discovery evidence, not authority
//! to mint a session: callers must still start through the persisted session
//! path because a release may be suspended or retired after catalog discovery.

use crate::postgres_instrument_release::{
    load_published_instrument_release, InstrumentReleaseQueryError,
    PublishedInstrumentReleaseSnapshot,
};
use postgres::GenericClient;

/// List persisted releases that are currently eligible to start new sessions.
///
/// Rows are ordered by instrument family, exact locale, then release reference
/// so clients can present a deterministic catalog. Every row is reloaded through
/// [`load_published_instrument_release`]; malformed or concurrently ineligible
/// evidence therefore fails the whole read instead of returning a partial list.
/// This function does not choose locale fallbacks and does not perform scoring.
///
/// # Errors
///
/// Returns [`InstrumentReleaseQueryError`] if `PostgreSQL` cannot execute the
/// query or if any selected row is missing, no longer published, or cannot be
/// reconstructed as an immutable published release.
pub fn list_startable_instrument_releases(
    client: &mut impl GenericClient,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, InstrumentReleaseQueryError> {
    let rows = client.query(
        "SELECT release_ref, locale \
         FROM instrument_release \
         WHERE publication_state = $1 \
         ORDER BY instrument_ref, locale, release_ref",
        &[&"published"],
    )?;

    let mut releases = Vec::with_capacity(rows.len());
    for row in rows {
        let release_ref: String = row.get("release_ref");
        let locale: String = row.get("locale");
        releases.push(load_published_instrument_release(
            client,
            &release_ref,
            &locale,
        )?);
    }
    Ok(releases)
}
