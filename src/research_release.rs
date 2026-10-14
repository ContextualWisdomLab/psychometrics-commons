//! Bounded product evidence for Research Commons release approval.
//!
//! The accepted Research Commons governance requires an immutable snapshot, declared
//! research scope, privacy/scientific review evidence, licensing, measurement provenance,
//! access classification, citation metadata, and independent approval before release.
//! This module validates those references and rejects public fixtures that still carry
//! restricted identity, authentication, credential, or internal-location fields. It does
//! not publish artifacts or call the external research catalog.

use crate::reference::{is_default_ignorable_identifier_character, normalized_reference};
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

/// Immutable evidence that passed the product-side research-release gate.
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
/// Operational identity, authentication, credential, and restricted internal-location
/// names are not. Structured values must be flattened or independently scanned before
/// this boundary accepts them.
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
/// already held for the people represented by the fixture. At least one effective exact
/// nonblank identity must be supplied so an omitted or malformed inventory cannot be
/// mistaken for a clean scan. Blank placeholders are ignored. This boundary never queries
/// another service's application database.
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
    /// A published column name is blank or contains a forbidden identity/security marker.
    ForbiddenColumn,
    /// No published columns were supplied, so there is no fixture to verify.
    EmptyFixture,
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
                "remove blank, restricted identity, authentication, credential, or internal-location columns from the public release fixture"
            }
            Self::EmptyFixture => {
                "supply at least one published column before treating a public release fixture scan as clean"
            }
            Self::IdentityInventoryUnavailable => {
                "supply an authorized exact restricted-identity inventory before packaging the public release fixture"
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
    "pseudonym_key",
    "pseudonym_key_version",
    "subject_ref",
    "assessment_session_ref",
    "session_ref",
    "session_id",
    "result_ref",
    "response_ref",
    "item_delivery_ref",
    "scoring_request_ref",
    "access_token",
    "auth_token",
    "refresh_token",
    "api_key",
    "client_secret",
    "jwt",
    "session_cookie",
    "cookie_header",
    "set_cookie",
    "database_url",
    "database_dsn",
    "database_password",
    "database_host",
    "database_hostname",
    "database_port",
    "db_host",
    "db_endpoint",
    "object_store_access_key",
    "object_store_secret_key",
    "object_store_endpoint",
    "object_store_host",
    "object_store_bucket",
    "s3_endpoint",
    "s3_bucket",
];

const FORBIDDEN_RESEARCH_NAMESPACE_PREFIX_MARKERS: &[&str] = &[
    "assessment",
    "auth",
    "credential",
    "database",
    "identity",
    "itemdelivery",
    "keyverse",
    "linkage",
    "linked",
    "objectstore",
    "operational",
    "participant",
    "password",
    "pseudonym",
    "response",
    "result",
    "scoringrequest",
    "secret",
    "session",
    "subject",
    "token",
];

const FORBIDDEN_COMPOUND_IDENTITY_MARKERS: &[&str] = &[
    "identity",
    "keyverse",
    "linkage",
    "linked",
    "operational",
    "pseudonym",
    "subject",
];

const FORBIDDEN_CREDENTIAL_WORDS: &[&str] = &[
    "auth",
    "credential",
    "credentials",
    "key",
    "keys",
    "password",
    "passwords",
    "secret",
    "secrets",
    "token",
    "tokens",
];

const ALLOWED_AUTHOR_RESEARCH_NAMESPACE_PREFIXES: &[&str] =
    &["author", "authors", "authoredby", "authoringtool"];

/// Reject a public-release fixture that still carries restricted identity or secrets.
///
/// Call this before packaging a public or catalog-facing release.
///
/// - Publish only nonblank ASCII column names. Blank or non-ASCII aliases fail closed before
///   normalization.
/// - `research_participant_ref` is the allowed public research identity namespace. Clear
///   export, staging, and author-metadata prefixes remain allowed unless they also carry a
///   restricted identity, authentication, credential, or internal-location marker.
/// - Operational product-resource references, including assessment session, result, response,
///   item-delivery, and scoring-request references, fail closed instead of linking a research
///   row back to the hosted product lifecycle.
/// - Identity, authentication, credential, and internal-location column names fail closed
///   even when aliases add transport prefixes, suffixes, separators, or inserted digits.
/// - Generic session identifiers, JWT bearer material, session-cookie, and HTTP cookie-header
///   credential columns fail closed, while ordinary cookie-governance metadata is not rejected
///   merely for containing the word `cookie`.
/// - Supply at least one published column and an effective product-authorized exact
///   restricted-identity inventory. A malformed nonblank inventory entry fails closed; blank
///   placeholders are ignored. An empty or unavailable inventory is never clean-release evidence.
/// - Flat cell values are checked for authorized restricted identities even when a known
///   identity is embedded in otherwise flat text. Unicode default-ignorable formatting
///   characters do not make an otherwise matching restricted identity publishable. Object and
///   array values must be flattened or independently privacy-scanned before packaging.
///
/// This boundary never queries Keyverse, a linkage service, or another service's application
/// database to supplement the caller's authorized inventory.
///
/// # Errors
///
/// Returns [`PublicReleaseLeakageError`] when no published columns are supplied, a blank or
/// forbidden column is present, the effective restricted-identity inventory is unavailable,
/// a structured cell cannot be inspected safely, or a cell value is an operational
/// participant, Keyverse subject, restricted linkage identity, or linkage-key version.
pub fn scan_public_release_fixture(
    columns: &[PublicReleaseFixtureColumn<'_>],
    restricted: RestrictedReleaseIdentities<'_>,
) -> Result<(), PublicReleaseLeakageError> {
    if columns.is_empty() {
        return Err(PublicReleaseLeakageError::EmptyFixture);
    }

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
    let mut has_effective_identity = false;
    for identity in [
        restricted.operational_participant_refs,
        restricted.keyverse_subject_refs,
        restricted.linkage_refs,
        restricted.linkage_key_versions,
    ]
    .into_iter()
    .flatten()
    {
        if identity.trim().is_empty() {
            continue;
        }
        let Some(normalized) = normalized_reference(identity) else {
            return false;
        };
        if normalized != *identity {
            return false;
        }
        has_effective_identity = true;
    }
    has_effective_identity
}

fn forbidden_public_release_column(column_name: &str) -> bool {
    if column_name.trim().is_empty() || !column_name.is_ascii() {
        return true;
    }

    let normalized = normalize_public_release_column(column_name);
    let compact = compact_public_release_column(&normalized);
    let research_namespace = compact_public_release_column("research_participant_ref");

    if compact == research_namespace {
        return false;
    }

    if let Some(prefix) = compact.strip_suffix(&research_namespace) {
        if ALLOWED_AUTHOR_RESEARCH_NAMESPACE_PREFIXES.contains(&prefix) {
            return false;
        }
        return contains_forbidden_public_release_marker(prefix)
            || contains_forbidden_research_namespace_prefix_marker(prefix)
            || contains_forbidden_credential_word(prefix, prefix)
            || contains_forbidden_compound_identity_marker(prefix);
    }

    contains_forbidden_public_release_marker(&compact)
        || contains_forbidden_credential_word(&normalized, &compact)
        || contains_forbidden_compound_identity_marker(&compact)
        || FORBIDDEN_RESEARCH_NAMESPACE_PREFIX_MARKERS
            .iter()
            .any(|marker| compact == *marker)
}

fn contains_forbidden_public_release_marker(compact: &str) -> bool {
    FORBIDDEN_PUBLIC_RELEASE_COLUMNS.iter().any(|forbidden| {
        let forbidden = compact_public_release_column(forbidden);
        compact.contains(forbidden.as_str())
    })
}

fn contains_forbidden_research_namespace_prefix_marker(compact: &str) -> bool {
    FORBIDDEN_RESEARCH_NAMESPACE_PREFIX_MARKERS
        .iter()
        .any(|marker| compact.contains(marker))
}

fn contains_forbidden_compound_identity_marker(compact: &str) -> bool {
    FORBIDDEN_COMPOUND_IDENTITY_MARKERS
        .iter()
        .any(|marker| compact.contains(marker))
}

fn contains_forbidden_credential_word(normalized: &str, compact: &str) -> bool {
    let mut words = normalized
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|word| !word.is_empty());
    let first_word = words.next();
    let author_metadata_word = first_word
        .is_some_and(|word| ["author", "authors", "authored", "authoring"].contains(&word));

    if first_word.is_some_and(|word| FORBIDDEN_CREDENTIAL_WORDS.contains(&word))
        || words.any(|word| FORBIDDEN_CREDENTIAL_WORDS.contains(&word))
    {
        return true;
    }

    [
        "credential",
        "credentials",
        "password",
        "passwords",
        "secret",
        "secrets",
        "token",
        "tokens",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
        || compact.ends_with("key")
        || compact.ends_with("keys")
        || (compact.starts_with("auth") && !author_metadata_word)
}

fn compact_public_release_column(column_name: &str) -> String {
    column_name
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect()
}

/// Fold ASCII case and camelCase/PascalCase acronym boundaries so CSV/JSON export aliases match the denylist.
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
    let cell = cell.trim_start_matches(|character: char| {
        character.is_whitespace() || is_default_ignorable_identifier_character(character)
    });
    cell.starts_with('{') || cell.starts_with('[')
}

fn matches_restricted_identity(cell: &str, restricted_identities: &[&str]) -> bool {
    let cell = cell.trim();
    if cell.is_empty() {
        return false;
    }

    if restricted_identities.iter().any(|identity| {
        let identity = identity.trim();
        !identity.is_empty() && cell.contains(identity)
    }) {
        return true;
    }

    if !cell.chars().any(is_default_ignorable_identifier_character) {
        return false;
    }

    let visible_cell: String = cell
        .chars()
        .filter(|character| !is_default_ignorable_identifier_character(*character))
        .collect();
    restricted_identities.iter().any(|identity| {
        let identity = identity.trim();
        !identity.is_empty() && visible_cell.contains(identity)
    })
}

/// Fail-closed research-release gate error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResearchReleaseGateError {
    /// A required reference was blank, numeric-like, unsafe, or not the exact issued spelling.
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
                "research release references must use the exact opaque non-numeric spelling"
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
/// Every reference must already use its exact issued spelling. The gate does
/// not trim an alias and silently store or compare a different identity.
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
    let normalized =
        normalized_reference(reference).ok_or(ResearchReleaseGateError::InvalidReference)?;
    if normalized != reference {
        return Err(ResearchReleaseGateError::InvalidReference);
    }
    Ok(normalized)
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
