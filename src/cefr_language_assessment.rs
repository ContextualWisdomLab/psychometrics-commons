//! Product-owned boundary for the CWL CEFR language-assessment profile.
//!
//! The shared JSON schemas and executable validator remain owned by
//! `learning-interoperability-contracts`. This module pins that review-only
//! Draft head and validates product-owned identity, domain, claim, and
//! evidence bindings without copying descriptor text or implementing scoring.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Versioned profile namespace owned by the shared CWL CEFR contract.
pub const CEFR_LANGUAGE_ASSESSMENT_PROFILE_VERSION: &str = "cwl_cefr_language_assessment/v1";

/// Version of the shared CWL CEFR result-snapshot envelope.
pub const CEFR_LANGUAGE_ASSESSMENT_CONTRACT_VERSION: &str =
    "cwl_cefr_language_assessment/result_snapshot/v1";

/// Upstream repository that owns the shared CEFR profile schemas and validator.
pub const CEFR_LANGUAGE_ASSESSMENT_CONTRACT_REPOSITORY: &str =
    "ContextualWisdomLab/learning-interoperability-contracts";

/// Exact upstream commit pinned for the review-only consumer.
pub const CEFR_LANGUAGE_ASSESSMENT_DRAFT_COMMIT: &str = "ec9a2aa312ccd078da7b76c5325c34f1e1eb2482";

/// SHA-256 of the pinned assessment-blueprint schema bytes.
pub const CEFR_ASSESSMENT_BLUEPRINT_SCHEMA_DIGEST: &str =
    "sha256:adf9c271f2a86208e50308b06b3a7172fedd5af0fe020c0d1d995ec62b790594";

/// SHA-256 of the pinned task-specification schema bytes.
pub const CEFR_TASK_SPECIFICATION_SCHEMA_DIGEST: &str =
    "sha256:3b7ed7147a3f8f7b1d5a6a95b1c299c27ebbb657d51606986d5f3f20a1f3eb36";

/// SHA-256 of the pinned CEFR result-snapshot schema bytes.
pub const CEFR_RESULT_SNAPSHOT_SCHEMA_DIGEST: &str =
    "sha256:e834719420429228c07ec2febacf5cad27aab93c5704c52a3bab6e1251034726";

const REQUIRED_DOMAINS: [CefrActivityDomain; 4] = [
    CefrActivityDomain::ReadingReception,
    CefrActivityDomain::ListeningReception,
    CefrActivityDomain::WrittenProduction,
    CefrActivityDomain::SpokenProduction,
];

/// Immutable source and schema identities for one shared-contract consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CefrContractPin {
    repository: &'static str,
    commit: &'static str,
    assessment_blueprint_schema_digest: &'static str,
    task_specification_schema_digest: &'static str,
    result_snapshot_schema_digest: &'static str,
}

impl CefrContractPin {
    /// Return the exact Draft PR #5 pin used for review.
    #[must_use]
    pub const fn draft_pr_five_review_pin() -> Self {
        Self {
            repository: CEFR_LANGUAGE_ASSESSMENT_CONTRACT_REPOSITORY,
            commit: CEFR_LANGUAGE_ASSESSMENT_DRAFT_COMMIT,
            assessment_blueprint_schema_digest: CEFR_ASSESSMENT_BLUEPRINT_SCHEMA_DIGEST,
            task_specification_schema_digest: CEFR_TASK_SPECIFICATION_SCHEMA_DIGEST,
            result_snapshot_schema_digest: CEFR_RESULT_SNAPSHOT_SCHEMA_DIGEST,
        }
    }

    /// Return the source repository for the pinned contract.
    #[must_use]
    pub const fn repository(self) -> &'static str {
        self.repository
    }

    /// Return the immutable source commit for the pinned contract.
    #[must_use]
    pub const fn commit(self) -> &'static str {
        self.commit
    }

    /// Return the pinned assessment-blueprint schema digest.
    #[must_use]
    pub const fn assessment_blueprint_schema_digest(self) -> &'static str {
        self.assessment_blueprint_schema_digest
    }

    /// Return the pinned task-specification schema digest.
    #[must_use]
    pub const fn task_specification_schema_digest(self) -> &'static str {
        self.task_specification_schema_digest
    }

    /// Return the pinned result-snapshot schema digest.
    #[must_use]
    pub const fn result_snapshot_schema_digest(self) -> &'static str {
        self.result_snapshot_schema_digest
    }
}

/// One of the four initial English placement reporting domains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CefrActivityDomain {
    /// Reading reception domain.
    ReadingReception,
    /// Listening reception domain.
    ListeningReception,
    /// Written production domain.
    WrittenProduction,
    /// Spoken production domain.
    SpokenProduction,
}

impl CefrActivityDomain {
    /// Return the stable shared-contract domain code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ReadingReception => "reading_reception",
            Self::ListeningReception => "listening_reception",
            Self::WrittenProduction => "written_production",
            Self::SpokenProduction => "spoken_production",
        }
    }
}

/// Claim status permitted by the initial English placement profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CefrClaimStatus {
    /// Research-only evidence without an operational CEFR interpretation claim.
    Experimental,
    /// CEFR construct and exact profile references exist without linking evidence.
    CefrAligned,
    /// Standard-setting and empirical linking evidence is pinned.
    CefrLinked,
    /// A governed certification authority and policy are also pinned.
    CertificationDecision,
}

/// Fail-closed error for the product-owned CEFR contract boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CefrProfileError {
    /// A required identity reference was blank, numeric-like, or unsafe.
    InvalidReference,
    /// The supplied contract version is not the pinned result-snapshot version.
    ContractVersionMismatch,
    /// The result was bound to a different immutable assessment blueprint.
    BlueprintMismatch,
    /// The result schema digest is not the pinned immutable schema.
    ResultSchemaDigestMismatch,
    /// No external executable-schema-validation evidence was supplied.
    MissingSchemaValidationReference,
    /// The measured-domain references are duplicated or outside the profile.
    InvalidRequiredDomainSet,
    /// The claim would require linking or certification evidence not present here.
    UnsupportedClaimStatus,
    /// Overall reporting is not authorized by this profile-only Draft consumer.
    OverallReportingDisabled,
}

impl Display for CefrProfileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidReference => "CEFR profile references must be opaque durable values",
            Self::ContractVersionMismatch => "CEFR result contract version does not match the pinned profile",
            Self::BlueprintMismatch => "CEFR result is bound to a different assessment blueprint",
            Self::ResultSchemaDigestMismatch => "CEFR result schema digest does not match the pinned schema",
            Self::MissingSchemaValidationReference => {
                "CEFR result requires external executable schema-validation evidence"
            }
            Self::InvalidRequiredDomainSet => {
                "CEFR result measured-domain references must be unique required placement domains"
            }
            Self::UnsupportedClaimStatus => {
                "CEFR placement profile accepts aligned claims only until linking evidence is governed"
            }
            Self::OverallReportingDisabled => {
                "CEFR overall reporting is disabled until the exact blueprint authorizes it"
            }
        };
        formatter.write_str(message)
    }
}

impl Error for CefrProfileError {}

/// Product-owned references for the initial English A1-B2 placement profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishA1B2PlacementProfile {
    contract_pin: CefrContractPin,
    instrument_release_ref: String,
    assessment_blueprint_ref: String,
    scoring_profile_ref: String,
    cut_score_revision_ref: String,
}

impl EnglishA1B2PlacementProfile {
    /// Create the review-only English A1-B2 profile reference bundle.
    ///
    /// This constructor stores only product-owned opaque references. The
    /// upstream schemas, descriptor content, task payloads, responses, audio,
    /// and scoring calculations remain outside this repository.
    ///
    /// # Errors
    ///
    /// Returns [`CefrProfileError::InvalidReference`] when any supplied
    /// reference is blank, numeric-like, or unsafe.
    pub fn new(
        instrument_release_ref: &str,
        assessment_blueprint_ref: &str,
        scoring_profile_ref: &str,
        cut_score_revision_ref: &str,
    ) -> Result<Self, CefrProfileError> {
        Ok(Self {
            contract_pin: CefrContractPin::draft_pr_five_review_pin(),
            instrument_release_ref: required_reference(instrument_release_ref)?.to_owned(),
            assessment_blueprint_ref: required_reference(assessment_blueprint_ref)?.to_owned(),
            scoring_profile_ref: required_reference(scoring_profile_ref)?.to_owned(),
            cut_score_revision_ref: required_reference(cut_score_revision_ref)?.to_owned(),
        })
    }

    /// Return the review-only upstream contract pin.
    #[must_use]
    pub const fn contract_pin(&self) -> CefrContractPin {
        self.contract_pin
    }

    /// Return the immutable product instrument-release reference.
    #[must_use]
    pub fn instrument_release_ref(&self) -> &str {
        &self.instrument_release_ref
    }

    /// Return the exact assessment-blueprint reference.
    #[must_use]
    pub fn assessment_blueprint_ref(&self) -> &str {
        &self.assessment_blueprint_ref
    }

    /// Return the exact numerical scoring-profile reference.
    #[must_use]
    pub fn scoring_profile_ref(&self) -> &str {
        &self.scoring_profile_ref
    }

    /// Return the exact cut-score revision reference.
    #[must_use]
    pub fn cut_score_revision_ref(&self) -> &str {
        &self.cut_score_revision_ref
    }

    /// Return the four required domains in their stable contract order.
    #[must_use]
    pub const fn required_domains(&self) -> &'static [CefrActivityDomain; 4] {
        &REQUIRED_DOMAINS
    }

    /// Return the only claim status currently authorized by this profile.
    #[must_use]
    pub const fn claim_status(&self) -> CefrClaimStatus {
        CefrClaimStatus::CefrAligned
    }

    /// Validate one externally schema-checked immutable result binding.
    ///
    /// The upstream executable validator must run before this boundary and
    /// its durable evidence reference must be supplied as
    /// `schema_validation_ref`. This method does not parse or reproduce the
    /// upstream JSON schemas.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the contract version, blueprint, schema
    /// digest, validation evidence, required domains, claim status, or overall
    /// reporting policy does not match this profile.
    pub fn validate_result(
        &self,
        input: CefrResultValidationInput<'_>,
    ) -> Result<(), CefrProfileError> {
        if input.contract_version != CEFR_LANGUAGE_ASSESSMENT_CONTRACT_VERSION {
            return Err(CefrProfileError::ContractVersionMismatch);
        }
        if input.assessment_blueprint_ref != self.assessment_blueprint_ref {
            return Err(CefrProfileError::BlueprintMismatch);
        }
        if input.result_schema_digest != self.contract_pin.result_snapshot_schema_digest() {
            return Err(CefrProfileError::ResultSchemaDigestMismatch);
        }
        required_reference(input.result_ref)?;
        required_reference(input.schema_validation_ref)
            .map_err(|_| CefrProfileError::MissingSchemaValidationReference)?;
        let has_duplicate_domain = input
            .measured_domains
            .iter()
            .enumerate()
            .any(|(index, domain)| input.measured_domains[..index].contains(domain));
        if has_duplicate_domain {
            return Err(CefrProfileError::InvalidRequiredDomainSet);
        }
        if input.claim_status != self.claim_status() {
            return Err(CefrProfileError::UnsupportedClaimStatus);
        }
        if input.overall_result_reported {
            return Err(CefrProfileError::OverallReportingDisabled);
        }
        Ok(())
    }
}

/// Identity and evidence fields supplied by an upstream-validated result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CefrResultValidationInput<'a> {
    /// Immutable result reference generated by Psychometrics Commons.
    pub result_ref: &'a str,
    /// Shared contract version declared by the result envelope.
    pub contract_version: &'a str,
    /// Blueprint reference declared by the result envelope.
    pub assessment_blueprint_ref: &'a str,
    /// SHA-256 digest of the result-snapshot schema used by validation.
    pub result_schema_digest: &'a str,
    /// Opaque evidence reference emitted by the executable upstream validator.
    pub schema_validation_ref: &'a str,
    /// Domains whose result evidence has status `measured`.
    pub measured_domains: &'a [CefrActivityDomain],
    /// Claim status declared by the result envelope.
    pub claim_status: CefrClaimStatus,
    /// Whether the result contains a reported overall level.
    pub overall_result_reported: bool,
}

fn required_reference(reference: &str) -> Result<&str, CefrProfileError> {
    normalized_reference(reference).ok_or(CefrProfileError::InvalidReference)
}
