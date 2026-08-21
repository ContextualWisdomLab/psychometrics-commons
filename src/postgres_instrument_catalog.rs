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
use crate::reference::normalized_reference;
use postgres::{GenericClient, Row};

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
    reload_catalog_rows(client, rows)
}

/// List currently startable releases for one exact opaque instrument family.
///
/// The family filter is applied inside `PostgreSQL`, rather than by loading the
/// complete public catalog into transport code. Results are ordered by exact
/// locale and release reference. No locale fallback is inferred: callers that
/// require one locale must select an exact locale from the returned evidence and
/// session start must still re-check the persisted release under its start lock.
///
/// # Errors
///
/// Returns [`InstrumentReleaseQueryError::InvalidReference`] when
/// `instrument_ref` is blank, numeric-like, unsafe, or would change under the
/// product's canonical opaque-reference normalization. Other query/reload errors
/// use the same fail-closed semantics as [`list_startable_instrument_releases`].
pub fn list_startable_instrument_releases_for_family(
    client: &mut impl GenericClient,
    instrument_ref: &str,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, InstrumentReleaseQueryError> {
    let canonical = normalized_reference(instrument_ref)
        .filter(|normalized| *normalized == instrument_ref)
        .ok_or(InstrumentReleaseQueryError::InvalidReference)?;
    let rows = client.query(
        "SELECT release_ref, locale \
         FROM instrument_release \
         WHERE publication_state = $1 AND instrument_ref = $2 \
         ORDER BY locale, release_ref",
        &[&"published", &canonical],
    )?;
    reload_catalog_rows(client, rows)
}

fn reload_catalog_rows(
    client: &mut impl GenericClient,
    rows: Vec<Row>,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, InstrumentReleaseQueryError> {
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
