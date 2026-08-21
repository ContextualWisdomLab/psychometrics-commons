//! Bounded product evidence for Research Commons release approval.
//!
//! The accepted Research Commons governance requires an immutable snapshot, declared
//! research scope, privacy/scientific review evidence, licensing, measurement provenance,
//! access classification, citation metadata, and independent approval before release.
//! This module validates those references and rejects public fixtures that still carry
//! operational, Keyverse, or restricted-linkage identifiers. It does not publish artifacts
//! or call the external research catalog.

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

/// One column in a public research-release fixture.
///
/// A buyer packaging a public release passes the column name the fixture would
/// publish and the cell values in that column. Research identities are allowed.
/// Operational, Keyverse, and restricted-linkage names are not. Structured values
/// must be flattened or independently scanned before this boundary accepts them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicReleaseFixtureColumn<'a> {
    /// Published column name.
    pub column_name: &'a str,
    /// Cell values that would be written under that column.
    pub cell_values: &'a [&'a str],
}

/// Identities that a public release fixture must not carry.
///
/// Pass the product-authorized operational, Keyverse, and restricted-linkage values
/// already held for the people represented by the fixture. At least one effective
/// nonblank identity must be supplied so an omitted inventory cannot be mistaken for
/// a clean scan. This boundary never queries another service's application database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestrictedReleaseIdentities<'a> {
    /// Operational assessment participant references.
    pub operational_participant_refs: &'a [&'a str],
    /// Keyverse subject references.
    pub keyverse_subject_refs: &'a [&'a str],
    /// Restricted linkage identities.
    pub linkage_refs: &'a [&'a str],
    /// Restricted linkage-key versions.
    pub linkage_key_versions: &'a [&'a str],
}

/// Fail-closed public-release identifier leakage or missing scan authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicReleaseLeakageError {
    /// A published column name is an operational, Keyverse, or linkage field.
    ForbiddenColumn,
    /// No effective restricted-identity inventory was supplied for the scan.
    IdentityInventoryUnavailable,
    /// A cell contains structured data this flat scanner cannot inspect safely.
    StructuredValueUnsupported,
    /// A cell value is an operational participant identifier.
    OperationalParticipant,
    /// A cell value is a Keyverse subject identifier.
    KeyverseSubject,
    /// A cell value is a restricted linkage identity or linkage-key version.
    RestrictedLinkage,
}

impl Display for PublicReleaseLeakageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ForbiddenColumn => {
                "remove operational, Keyverse, or restricted-linkage columns from the public release fixture"
            }
            Self::IdentityInventoryUnavailable => {
                "supply an authorized restricted-identity inventory before packaging the public release fixture"
            }
            Self::StructuredValueUnsupported => {
                "flatten or independently scan structured public-release values before packaging the fixture"
            }
            Self::OperationalParticipant => {
                "remove operational participant identifiers from the public release fixture"
            }
            Self::KeyverseSubject => {
                "remove Keyverse subject identifiers from the public release fixture"
            }
            Self::RestrictedLinkage => {
                "remove restricted linkage identifiers and linkage-key versions from the public release fixture"
            }
        })
    }
}

impl Error for PublicReleaseLeakageError {}

const FORBIDDEN_PUBLIC_RELEASE_COLUMNS: &[&str] = &[
    "assessment_participant_ref",
    "identity_subject_ref",
    "keyverse_subject",
    "keyverse_subject_ref",
    "linkage_key",
    "linkage_key_version",
    "linkage_ref",
    "linked_subject_ref",
    "operational_participant_ref",
    "participant_id",
    "participant_ref",
    "pseudonym_key_version",
    "subject_ref",
];

/// Reject a public-release fixture that still carries restricted identity.
///
/// Call this before packaging a public or catalog-facing release. Authorized
/// research that needs the restricted mapping keeps those values outside this
/// fixture. A column in the `research_participant_ref` namespace is allowed,
/// including clear export or staging prefixes and supported separator variants.
/// Restricted identity names remain forbidden when transport prefixes, suffixes,
/// or punctuation/whitespace separators are added around them. The caller must also
/// supply an effective product-authorized identity inventory; the scanner fails
/// closed rather than treating an omitted inventory as evidence that the fixture is
/// clean. Object or array cell values are not parsed here: callers must flatten them
/// or prove a separate structured-value privacy scan before packaging.
///
/// # Errors
///
/// Returns [`PublicReleaseLeakageError`] when a forbidden column is present, the
/// effective restricted-identity inventory is unavailable, a structured cell cannot
/// be inspected safely, or a cell value is an operational participant, Keyverse
/// subject, restricted linkage identity, or linkage-key version.
pub fn scan_public_release_fixture(
    columns: &[PublicReleaseFixtureColumn<'_>],
    restricted: RestrictedReleaseIdentities<'_>,
) -> Result<(), PublicReleaseLeakageError> {
    for column in columns {
        if forbidden_public_release_column(column.column_name) {
            return Err(PublicReleaseLeakageError::ForbiddenColumn);
        }
    }

    if !has_effective_restricted_identity_inventory(restricted) {
        return Err(PublicReleaseLeakageError::IdentityInventoryUnavailable);
    }

    for column in columns {
        for cell in column.cell_values {
            if structured_public_release_cell(cell) {
                return Err(PublicReleaseLeakageError::StructuredValueUnsupported);
            }
            if matches_restricted_identity(cell, restricted.operational_participant_refs) {
                return Err(PublicReleaseLeakageError::OperationalParticipant);
            }
            if matches_restricted_identity(cell, restricted.keyverse_subject_refs) {
                return Err(PublicReleaseLeakageError::KeyverseSubject);
            }
            if matches_restricted_identity(cell, restricted.linkage_refs)
                || matches_restricted_identity(cell, restricted.linkage_key_versions)
            {
                return Err(PublicReleaseLeakageError::RestrictedLinkage);
            }
        }
    }
    Ok(())
}

fn has_effective_restricted_identity_inventory(
    restricted: RestrictedReleaseIdentities<'_>,
) -> bool {
    [
        restricted.operational_participant_refs,
        restricted.keyverse_subject_refs,
        restricted.linkage_refs,
        restricted.linkage_key_versions,
    ]
    .into_iter()
    .flatten()
    .any(|identity| !identity.trim().is_empty())
}

fn forbidden_public_release_column(column_name: &str) -> bool {
    let normalized = normalize_public_release_column(column_name);
    let compact = compact_public_release_column(&normalized);
    let research_namespace = compact_public_release_column("research_participant_ref");
    if compact == research_namespace || compact.ends_with(&research_namespace) {
        return false;
    }

    FORBIDDEN_PUBLIC_RELEASE_COLUMNS
        .iter()
        .any(|forbidden| compact.contains(&compact_public_release_column(forbidden)))
}

fn compact_public_release_column(column_name: &str) -> String {
    column_name
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

/// Fold ASCII case and camelCase/PascalCase acronym boundaries so CSV/JSON export aliases match the denylist.
///
/// `researchParticipantRef` becomes `research_participant_ref` and stays
/// allowed. `assessmentPARTICIPANTRef` becomes `assessment_participant_ref`
/// and is rejected rather than bypassing the denylist through an uppercase run.
fn normalize_public_release_column(column_name: &str) -> String {
    let trimmed = column_name.trim();
    let mut normalized = String::with_capacity(trimmed.len() + 4);
    let mut previous: Option<char> = None;
    let mut characters = trimmed.chars().peekable();

    while let Some(current) = characters.next() {
        let next = characters.peek().copied();
        let starts_new_word = previous
            .is_some_and(|prior| prior.is_ascii_lowercase() || prior.is_ascii_digit())
            || (previous.is_some_and(|prior| prior.is_ascii_uppercase())
                && next.is_some_and(|following| following.is_ascii_lowercase()));

        if current.is_ascii_uppercase() && starts_new_word {
            normalized.push('_');
        }
        normalized.push(current.to_ascii_lowercase());
        previous = Some(current);
    }
    normalized
}

fn structured_public_release_cell(cell: &str) -> bool {
    let cell = cell.trim_start();
    cell.starts_with('{') || cell.starts_with('[')
}

fn matches_restricted_identity(cell: &str, restricted_identities: &[&str]) -> bool {
    let cell = cell.trim();
    !cell.is_empty()
        && restricted_identities
            .iter()
            .any(|identity| !identity.trim().is_empty() && identity.trim() == cell)
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
