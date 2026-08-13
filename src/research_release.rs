//! Bounded product evidence for Research Commons release approval.
//!
//! The accepted Research Commons governance requires an immutable snapshot, declared
//! research scope, privacy/scientific review evidence, licensing, measurement provenance,
//! access classification, citation metadata, and independent approval before release.
//! This module validates those references only. It does not publish artifacts or call the
//! external research catalog.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Explicit access classification for one research release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchAccessClass {
    /// Public release.
    Public,
    /// Controlled release.
    Controlled,
    /// Private release.
    Private,
    /// Embargoed release.
    Embargoed,
}

/// Borrowed evidence proposed for one research release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResearchReleaseCandidate<'a> {
    /// Stable opaque release reference.
    pub release_ref: &'a str,
    /// Immutable dataset snapshot reference.
    pub dataset_snapshot_ref: &'a str,
    /// Exact research scope reference.
    pub research_scope_ref: &'a str,
    /// Canonical SHA-256 digest of the release manifest.
    pub manifest_digest: &'a str,
    /// Privacy review evidence reference.
    pub privacy_review_ref: &'a str,
    /// Scientific review evidence reference.
    pub scientific_review_ref: &'a str,
    /// Complete metadata bundle reference.
    pub metadata_bundle_ref: &'a str,
    /// License and rights evidence reference.
    pub license_record_ref: &'a str,
    /// Measurement provenance evidence reference.
    pub measurement_provenance_ref: &'a str,
    /// Access-class approval evidence reference.
    pub access_approval_ref: &'a str,
    /// Citation metadata evidence reference.
    pub citation_metadata_ref: &'a str,
    /// Research-release approval evidence reference.
    pub release_approver_ref: &'a str,
    /// Ordinary administration reference used for separation-of-duties comparison.
    pub ordinary_admin_ref: &'a str,
    /// Number of unresolved release-blocking findings.
    pub unresolved_blocking_findings: usize,
    /// Explicit access classification.
    pub access_class: ResearchAccessClass,
}

/// Immutable normalized evidence that passed the product-side research-release gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedResearchRelease {
    release_ref: String,
    dataset_snapshot_ref: String,
    research_scope_ref: String,
    manifest_digest: String,
    privacy_review_ref: String,
    scientific_review_ref: String,
    metadata_bundle_ref: String,
    license_record_ref: String,
    measurement_provenance_ref: String,
    access_approval_ref: String,
    citation_metadata_ref: String,
    release_approver_ref: String,
    ordinary_admin_ref: String,
    access_class: ResearchAccessClass,
}

impl ApprovedResearchRelease {
    /// Return the research-release reference.
    #[must_use]
    pub fn release_ref(&self) -> &str {
        &self.release_ref
    }

    /// Return the immutable dataset snapshot reference.
    #[must_use]
    pub fn dataset_snapshot_ref(&self) -> &str {
        &self.dataset_snapshot_ref
    }

    /// Return the exact research scope reference.
    #[must_use]
    pub fn research_scope_ref(&self) -> &str {
        &self.research_scope_ref
    }

    /// Return the canonical release-manifest digest.
    #[must_use]
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    /// Return privacy-review evidence.
    #[must_use]
    pub fn privacy_review_ref(&self) -> &str {
        &self.privacy_review_ref
    }

    /// Return scientific-review evidence.
    #[must_use]
    pub fn scientific_review_ref(&self) -> &str {
        &self.scientific_review_ref
    }

    /// Return complete metadata-bundle evidence.
    #[must_use]
    pub fn metadata_bundle_ref(&self) -> &str {
        &self.metadata_bundle_ref
    }

    /// Return license and rights evidence.
    #[must_use]
    pub fn license_record_ref(&self) -> &str {
        &self.license_record_ref
    }

    /// Return measurement-provenance evidence.
    #[must_use]
    pub fn measurement_provenance_ref(&self) -> &str {
        &self.measurement_provenance_ref
    }

    /// Return access-class approval evidence.
    #[must_use]
    pub fn access_approval_ref(&self) -> &str {
        &self.access_approval_ref
    }

    /// Return citation metadata evidence.
    #[must_use]
    pub fn citation_metadata_ref(&self) -> &str {
        &self.citation_metadata_ref
    }

    /// Return research-release approval evidence.
    #[must_use]
    pub fn release_approver_ref(&self) -> &str {
        &self.release_approver_ref
    }

    /// Return the ordinary administration reference used for independence evidence.
    #[must_use]
    pub fn ordinary_admin_ref(&self) -> &str {
        &self.ordinary_admin_ref
    }

    /// Return the approved access classification.
    #[must_use]
    pub const fn access_class(&self) -> ResearchAccessClass {
        self.access_class
    }
}

/// Fail-closed research-release gate error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchReleaseGateError {
    /// A required reference was blank or numeric-like.
    InvalidReference,
    /// The manifest digest was not canonical lowercase SHA-256 evidence.
    InvalidManifestDigest,
    /// At least one release-blocking finding remains unresolved.
    UnresolvedBlockingFinding,
    /// Release approval was not independent from ordinary administration.
    SeparationOfDutiesViolation,
}

impl Display for ResearchReleaseGateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "research release references must be opaque non-numeric values"
            }
            Self::InvalidManifestDigest => {
                "research release manifest digest must be canonical sha256 evidence"
            }
            Self::UnresolvedBlockingFinding => "research release has unresolved blocking findings",
            Self::SeparationOfDutiesViolation => {
                "research release approver must be independent from ordinary administration"
            }
        })
    }
}

impl Error for ResearchReleaseGateError {}

/// Validate and freeze one set of Research Commons release evidence.
///
/// # Errors
///
/// Returns [`ResearchReleaseGateError`] for invalid references, invalid manifest identity,
/// unresolved blockers, or non-independent approval evidence.
pub fn approve_research_release(
    candidate: ResearchReleaseCandidate<'_>,
) -> Result<ApprovedResearchRelease, ResearchReleaseGateError> {
    let release_ref = required_reference(candidate.release_ref)?;
    let dataset_snapshot_ref = required_reference(candidate.dataset_snapshot_ref)?;
    let research_scope_ref = required_reference(candidate.research_scope_ref)?;
    let privacy_review_ref = required_reference(candidate.privacy_review_ref)?;
    let scientific_review_ref = required_reference(candidate.scientific_review_ref)?;
    let metadata_bundle_ref = required_reference(candidate.metadata_bundle_ref)?;
    let license_record_ref = required_reference(candidate.license_record_ref)?;
    let measurement_provenance_ref = required_reference(candidate.measurement_provenance_ref)?;
    let access_approval_ref = required_reference(candidate.access_approval_ref)?;
    let citation_metadata_ref = required_reference(candidate.citation_metadata_ref)?;
    let release_approver_ref = required_reference(candidate.release_approver_ref)?;
    let ordinary_admin_ref = required_reference(candidate.ordinary_admin_ref)?;

    if !valid_sha256_digest(candidate.manifest_digest) {
        return Err(ResearchReleaseGateError::InvalidManifestDigest);
    }
    if candidate.unresolved_blocking_findings != 0 {
        return Err(ResearchReleaseGateError::UnresolvedBlockingFinding);
    }
    if release_approver_ref == ordinary_admin_ref {
        return Err(ResearchReleaseGateError::SeparationOfDutiesViolation);
    }

    Ok(ApprovedResearchRelease {
        release_ref: release_ref.to_owned(),
        dataset_snapshot_ref: dataset_snapshot_ref.to_owned(),
        research_scope_ref: research_scope_ref.to_owned(),
        manifest_digest: candidate.manifest_digest.to_owned(),
        privacy_review_ref: privacy_review_ref.to_owned(),
        scientific_review_ref: scientific_review_ref.to_owned(),
        metadata_bundle_ref: metadata_bundle_ref.to_owned(),
        license_record_ref: license_record_ref.to_owned(),
        measurement_provenance_ref: measurement_provenance_ref.to_owned(),
        access_approval_ref: access_approval_ref.to_owned(),
        citation_metadata_ref: citation_metadata_ref.to_owned(),
        release_approver_ref: release_approver_ref.to_owned(),
        ordinary_admin_ref: ordinary_admin_ref.to_owned(),
        access_class: candidate.access_class,
    })
}

fn required_reference(reference: &str) -> Result<&str, ResearchReleaseGateError> {
    normalized_reference(reference).ok_or(ResearchReleaseGateError::InvalidReference)
}

fn valid_sha256_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
