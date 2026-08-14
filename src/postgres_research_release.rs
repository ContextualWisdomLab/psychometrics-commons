//! `PostgreSQL` 18 persistence for immutable approved Research Commons release evidence.
//!
//! This adapter stores only product-owned release approval evidence that already passed
//! [`crate::research_release::approve_research_release`]. It does not publish artifacts, expose
//! restricted research linkage, or call `semantic-data-portal`; public catalog registration
//! remains owned by that bounded context. Exact replay requires `READ COMMITTED`.

use crate::research_release::{ApprovedResearchRelease, ResearchAccessClass};
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const RESEARCH_RELEASE_MIGRATION: &str =
    include_str!("../migrations/0016_research_release_approval.sql");

/// Outcome of persisting one immutable approved research release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchReleasePersistenceDisposition {
    /// New approval evidence was inserted.
    Inserted,
    /// The exact immutable approval evidence was already present.
    Duplicate,
}

/// Fail-closed error for durable research-release approval persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResearchReleasePersistenceError {
    /// Release identity was replayed with different immutable approval evidence.
    ConflictingReplay,
    /// Persistence requires `PostgreSQL` `READ COMMITTED` isolation.
    UnsupportedIsolationLevel,
    /// `PostgreSQL` rejected or could not execute the persistence operation.
    Database(postgres::Error),
}

impl Display for ResearchReleasePersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ConflictingReplay => {
                "research release identity was replayed with conflicting approval evidence"
            }
            Self::UnsupportedIsolationLevel => {
                "research release persistence requires read committed isolation"
            }
            Self::Database(_) => "PostgreSQL research-release persistence failed",
        })
    }
}

impl Error for ResearchReleasePersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<postgres::Error> for ResearchReleasePersistenceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}

/// Apply the idempotent research-release approval migration.
///
/// # Errors
///
/// Returns the `PostgreSQL` error if the migration cannot be applied.
pub fn apply_research_release_migration(
    client: &mut impl postgres::GenericClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(RESEARCH_RELEASE_MIGRATION)
}

/// Persist immutable product-side approval evidence for one Research Commons release.
///
/// The accepted domain type has already validated opaque references, canonical manifest digest,
/// unresolved blockers, and separation of duties. Exact replay is idempotent. Reusing
/// `release_ref` with any different dataset snapshot, research scope, manifest digest, review,
/// rights, provenance, metadata, approval, citation, separation-of-duties, or access-class
/// evidence fails closed. Historical approval evidence is never updated in place. This function
/// does not imply that public catalog registration has occurred; that later handoff remains a
/// separate `semantic-data-portal` contract.
///
/// # Errors
///
/// Returns [`ResearchReleasePersistenceError`] for unsupported isolation, conflicting replay, or
/// a database failure.
pub fn persist_approved_research_release(
    transaction: &mut Transaction<'_>,
    release: &ApprovedResearchRelease,
) -> Result<ResearchReleasePersistenceDisposition, ResearchReleasePersistenceError> {
    require_read_committed(transaction)?;
    let access_class = access_class_name(release.access_class());
    let inserted = transaction.execute(
        "INSERT INTO research_release_approval (\
             research_release_ref, dataset_snapshot_ref, research_scope_ref, manifest_digest,\
             privacy_review_ref, scientific_review_ref, metadata_bundle_ref, license_record_ref,\
             measurement_provenance_ref, access_approval_ref, citation_metadata_ref,\
             release_approver_ref, ordinary_admin_ref, access_class\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
         ON CONFLICT (research_release_ref) DO NOTHING",
        &[
            &release.release_ref(),
            &release.dataset_snapshot_ref(),
            &release.research_scope_ref(),
            &release.manifest_digest(),
            &release.privacy_review_ref(),
            &release.scientific_review_ref(),
            &release.metadata_bundle_ref(),
            &release.license_record_ref(),
            &release.measurement_provenance_ref(),
            &release.access_approval_ref(),
            &release.citation_metadata_ref(),
            &release.release_approver_ref(),
            &release.ordinary_admin_ref(),
            &access_class,
        ],
    )?;
    if inserted == 1 {
        return Ok(ResearchReleasePersistenceDisposition::Inserted);
    }
    classify_existing_release(transaction, release, access_class)
}

fn classify_existing_release(
    transaction: &mut Transaction<'_>,
    release: &ApprovedResearchRelease,
    access_class: &str,
) -> Result<ResearchReleasePersistenceDisposition, ResearchReleasePersistenceError> {
    let row = transaction.query_one(
        "SELECT dataset_snapshot_ref, research_scope_ref, manifest_digest, privacy_review_ref,\
                scientific_review_ref, metadata_bundle_ref, license_record_ref,\
                measurement_provenance_ref, access_approval_ref, citation_metadata_ref,\
                release_approver_ref, ordinary_admin_ref, access_class \
         FROM research_release_approval WHERE research_release_ref = $1",
        &[&release.release_ref()],
    )?;
    let stored_dataset: String = row.get(0);
    let stored_scope: String = row.get(1);
    let stored_manifest: String = row.get(2);
    let stored_privacy: String = row.get(3);
    let stored_scientific: String = row.get(4);
    let stored_metadata: String = row.get(5);
    let stored_license: String = row.get(6);
    let stored_provenance: String = row.get(7);
    let stored_access_approval: String = row.get(8);
    let stored_citation: String = row.get(9);
    let stored_approver: String = row.get(10);
    let stored_admin: String = row.get(11);
    let stored_access_class: String = row.get(12);

    if stored_dataset == release.dataset_snapshot_ref()
        && stored_scope == release.research_scope_ref()
        && stored_manifest == release.manifest_digest()
        && stored_privacy == release.privacy_review_ref()
        && stored_scientific == release.scientific_review_ref()
        && stored_metadata == release.metadata_bundle_ref()
        && stored_license == release.license_record_ref()
        && stored_provenance == release.measurement_provenance_ref()
        && stored_access_approval == release.access_approval_ref()
        && stored_citation == release.citation_metadata_ref()
        && stored_approver == release.release_approver_ref()
        && stored_admin == release.ordinary_admin_ref()
        && stored_access_class == access_class
    {
        Ok(ResearchReleasePersistenceDisposition::Duplicate)
    } else {
        Err(ResearchReleasePersistenceError::ConflictingReplay)
    }
}

const fn access_class_name(access_class: ResearchAccessClass) -> &'static str {
    match access_class {
        ResearchAccessClass::Public => "public",
        ResearchAccessClass::Controlled => "controlled",
        ResearchAccessClass::Private => "private",
        ResearchAccessClass::Embargoed => "embargoed",
    }
}

fn require_read_committed(
    transaction: &mut Transaction<'_>,
) -> Result<(), ResearchReleasePersistenceError> {
    let row = transaction.query_one("SHOW transaction_isolation", &[])?;
    let isolation: String = row.get(0);
    if isolation == "read committed" {
        Ok(())
    } else {
        Err(ResearchReleasePersistenceError::UnsupportedIsolationLevel)
    }
}

#[cfg(test)]
mod helper_tests {
    use super::access_class_name;
    use crate::research_release::ResearchAccessClass;

    #[test]
    fn access_classes_map_to_persisted_vocabulary() {
        assert_eq!(access_class_name(ResearchAccessClass::Public), "public");
        assert_eq!(
            access_class_name(ResearchAccessClass::Controlled),
            "controlled"
        );
        assert_eq!(access_class_name(ResearchAccessClass::Private), "private");
        assert_eq!(
            access_class_name(ResearchAccessClass::Embargoed),
            "embargoed"
        );
    }
}