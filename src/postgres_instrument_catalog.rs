//! Durable startable-instrument catalog backed by product-owned `PostgreSQL` state.
//!
//! A persisted release is instrument-publication evidence already stored in the
//! product database. This adapter lists only releases whose persisted lifecycle
//! currently says `published`, then validates every selected row with the same
//! immutable stored-evidence reconstruction used by session start.
//!
//! Keyset order means rows are sorted by `(instrument_ref, locale, release_ref)`
//! and a continuation cursor resumes immediately after the last returned row,
//! rather than counting rows from the beginning again. A non-locking catalog read
//! does not reserve the release row while a participant is browsing. The returned
//! releases are therefore candidates only: publication state can change after the catalog read,
//! so the persisted session-start path locks and revalidates the exact release
//! before creating a session. This adapter neither authorizes nor creates sessions.

use crate::postgres_instrument_release::{
    published_instrument_release_snapshot_from_row, InstrumentReleaseQueryError,
    PublishedInstrumentReleaseSnapshot,
};
use postgres::GenericClient;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Maximum number of validated releases returned by one catalog query.
pub const STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE: usize = 100;
const STARTABLE_INSTRUMENT_RELEASE_FETCH_LIMIT: i64 = 101;

/// Opaque continuation state for the durable startable-instrument catalog.
///
/// Callers obtain cursors from [`StartableInstrumentReleasePage::next_cursor`]
/// instead of constructing database ordering keys themselves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartableInstrumentReleaseCursor {
    instrument_ref: String,
    locale: String,
    release_ref: String,
}

impl StartableInstrumentReleaseCursor {
    fn from_snapshot(snapshot: &PublishedInstrumentReleaseSnapshot) -> Self {
        let manifest = snapshot.manifest();
        Self {
            instrument_ref: manifest.instrument_ref().to_owned(),
            locale: manifest.locale().to_owned(),
            release_ref: manifest.release_ref().to_owned(),
        }
    }
}

/// One bounded, deterministically ordered page of currently published releases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartableInstrumentReleasePage {
    releases: Vec<PublishedInstrumentReleaseSnapshot>,
    next_cursor: Option<StartableInstrumentReleaseCursor>,
}

impl StartableInstrumentReleasePage {
    /// Return the validated release candidates in this page.
    #[must_use]
    pub fn releases(&self) -> &[PublishedInstrumentReleaseSnapshot] {
        &self.releases
    }

    /// Return the cursor required to continue when more rows were observed.
    #[must_use]
    pub const fn next_cursor(&self) -> Option<&StartableInstrumentReleaseCursor> {
        self.next_cursor.as_ref()
    }
}

/// Fail-closed error for durable catalog discovery.
#[derive(Debug)]
#[non_exhaustive]
pub enum StartableInstrumentCatalogError {
    /// A database or persisted-release validation failure occurred.
    Query(InstrumentReleaseQueryError),
    /// The compatibility all-at-once API would exceed one bounded page.
    PageRequired,
}

impl Display for StartableInstrumentCatalogError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Query(_) => "startable instrument catalog query failed",
            Self::PageRequired => {
                "startable instrument catalog exceeds one bounded page; use paged discovery"
            }
        })
    }
}

impl Error for StartableInstrumentCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query(error) => Some(error),
            Self::PageRequired => None,
        }
    }
}

impl From<InstrumentReleaseQueryError> for StartableInstrumentCatalogError {
    fn from(error: InstrumentReleaseQueryError) -> Self {
        Self::Query(error)
    }
}

/// Read one bounded page of persisted releases that may currently start sessions.
///
/// The database sorts releases by the stable key
/// `(instrument_ref, locale, release_ref)`. When a previous page supplied a
/// cursor, the next query resumes immediately after that page's last returned
/// release. This is keyset pagination: it avoids recounting earlier rows and
/// keeps the continuation rule tied to the same deterministic ordering.
///
/// The catalog query is non-locking, meaning it reads committed publication
/// evidence without reserving a release row while the caller browses. At most
/// [`STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE`] releases are returned, with a
/// continuation cursor only when the query observed another row. Because a
/// release can be suspended or retired after discovery, catalog results are
/// advisory. The persisted session-start path must lock and revalidate the exact
/// release immediately before creating a session.
///
/// Any malformed persisted evidence inside the returned page fails that page
/// instead of returning partial validated content. This function does not
/// authorize or create sessions, choose locale fallbacks, or perform scoring.
///
/// # Errors
///
/// Returns [`StartableInstrumentCatalogError::Query`] if `PostgreSQL` cannot
/// execute the query or if a selected row cannot be reconstructed as an
/// immutable published release.
pub fn list_startable_instrument_release_page(
    client: &mut impl GenericClient,
    after: Option<&StartableInstrumentReleaseCursor>,
) -> Result<StartableInstrumentReleasePage, StartableInstrumentCatalogError> {
    let rows = match after {
        Some(cursor) => client.query(
            "SELECT release_ref, instrument_ref, instrument_version_ref, construct_ref, \
                    item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
                    calibration_reference, norm_version_ref, narrative_version_ref, \
                    consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
                    publication_state, created_at_unix_ms \
             FROM instrument_release \
             WHERE publication_state = $1 \
               AND (instrument_ref, locale, release_ref) > ($2, $3, $4) \
             ORDER BY instrument_ref, locale, release_ref \
             LIMIT $5",
            &[
                &"published",
                &cursor.instrument_ref,
                &cursor.locale,
                &cursor.release_ref,
                &STARTABLE_INSTRUMENT_RELEASE_FETCH_LIMIT,
            ],
        ),
        None => client.query(
            "SELECT release_ref, instrument_ref, instrument_version_ref, construct_ref, \
                    item_version_refs, locale, assessment_spec_ref, scoring_version_ref, \
                    calibration_reference, norm_version_ref, narrative_version_ref, \
                    consent_requirement_refs, intended_use_ref, limitations_ref, content_digest, \
                    publication_state, created_at_unix_ms \
             FROM instrument_release \
             WHERE publication_state = $1 \
             ORDER BY instrument_ref, locale, release_ref \
             LIMIT $2",
            &[&"published", &STARTABLE_INSTRUMENT_RELEASE_FETCH_LIMIT],
        ),
    }
    .map_err(InstrumentReleaseQueryError::from)?;

    let has_more = rows.len() > STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE;
    let releases = rows
        .iter()
        .take(STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE)
        .map(published_instrument_release_snapshot_from_row)
        .collect::<Result<Vec<_>, InstrumentReleaseQueryError>>()?;
    let next_cursor = if has_more {
        releases
            .last()
            .map(StartableInstrumentReleaseCursor::from_snapshot)
    } else {
        None
    };

    Ok(StartableInstrumentReleasePage {
        releases,
        next_cursor,
    })
}

/// List all startable releases only when they fit in one bounded page.
///
/// This compatibility helper exists for callers that already expect a single
/// collection. It fails closed instead of silently truncating when more than
/// [`STARTABLE_INSTRUMENT_RELEASE_PAGE_SIZE`] releases are currently published.
/// New catalog transports should use [`list_startable_instrument_release_page`]
/// and carry the returned continuation cursor.
///
/// # Errors
///
/// Returns [`StartableInstrumentCatalogError::PageRequired`] when continuation
/// is required, or [`StartableInstrumentCatalogError::Query`] for database or
/// stored-evidence failures.
pub fn list_startable_instrument_releases(
    client: &mut impl GenericClient,
) -> Result<Vec<PublishedInstrumentReleaseSnapshot>, StartableInstrumentCatalogError> {
    let page = list_startable_instrument_release_page(client, None)?;
    if page.next_cursor.is_some() {
        return Err(StartableInstrumentCatalogError::PageRequired);
    }
    Ok(page.releases)
}
