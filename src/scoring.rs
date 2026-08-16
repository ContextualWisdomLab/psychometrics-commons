//! Scoring-dispatch contracts that pin immutable measurement provenance.
//!
//! This module does not calculate psychometric quantities. It defines the
//! hosted product boundary used to dispatch a completed response snapshot to a
//! versioned `fast-mlsirm`-compatible scoring implementation and to accept a
//! typed immutable result without collapsing missing outcomes into numeric zero.

use crate::reference::normalized_reference;
use crate::response::ResponseSnapshot;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

const SUPPORTED_OUTPUT_SCHEMA_VERSION: u16 = 1;
const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LENGTH: usize = 64;

/// Borrowed fields needed to dispatch one immutable response snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoringRequestInput<'a> {
    /// Opaque idempotent reference for the scoring request.
    pub scoring_request_ref: &'a str,
    /// Opaque reference expected to identify the supplied durable snapshot.
    pub response_snapshot_ref: &'a str,
    /// Exact reusable assessment-contract reference.
    pub assessment_spec_ref: &'a str,
    /// Exact published instrument-version reference.
    pub instrument_version_ref: &'a str,
    /// Exact scoring-policy or scoring-engine contract version.
    pub scoring_version_ref: &'a str,
    /// Exact calibration artifact reference or digest.
    pub calibration_reference: &'a str,
    /// Optional exact norm-version reference.
    pub norm_version_ref: Option<&'a str>,
    /// Requested major output schema understood by the product runtime.
    pub requested_output_schema_version: u16,
}

/// Immutable scoring request derived from a completed response snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoringRequest {
    request_ref: String,
    session_ref: String,
    response_snapshot_ref: String,
    assessment_spec_ref: String,
    instrument_version_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    requested_output_schema_version: u16,
}

impl ScoringRequest {
    /// Build a scoring request from a non-empty durable response snapshot.
    ///
    /// References are trimmed before becoming identity-bearing state. The
    /// caller-supplied snapshot reference must exactly match the durable
    /// reference embedded in `snapshot` after normalization.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] when a required or
    /// supplied optional reference is blank,
    /// [`ScoringContractError::UnboundResponseSnapshot`] when the snapshot has
    /// no durable identity, [`ScoringContractError::EmptyResponseSnapshot`] when
    /// the snapshot contains no response events,
    /// [`ScoringContractError::ResponseSnapshotMismatch`] when the supplied
    /// reference does not identify the snapshot, or
    /// [`ScoringContractError::UnsupportedOutputSchemaVersion`] when the
    /// requested schema major is not supported by this runtime.
    pub fn from_snapshot(
        snapshot: &ResponseSnapshot,
        input: ScoringRequestInput<'_>,
    ) -> Result<Self, ScoringContractError> {
        let request_ref = required_reference(input.scoring_request_ref)?;
        let requested_snapshot_ref = required_reference(input.response_snapshot_ref)?;
        let assessment_spec_ref = required_reference(input.assessment_spec_ref)?;
        let instrument_version_ref = required_reference(input.instrument_version_ref)?;
        let scoring_version_ref = required_reference(input.scoring_version_ref)?;
        let calibration_reference = required_reference(input.calibration_reference)?;
        let norm_version_ref = input.norm_version_ref.map(required_reference).transpose()?;

        let snapshot_ref = snapshot
            .snapshot_ref()
            .ok_or(ScoringContractError::UnboundResponseSnapshot)?;
        if snapshot.event_count() == 0 {
            return Err(ScoringContractError::EmptyResponseSnapshot);
        }
        if requested_snapshot_ref != snapshot_ref {
            return Err(ScoringContractError::ResponseSnapshotMismatch);
        }
        Self::from_validated_pins(ValidatedScoringPins {
            session_ref: snapshot.session_ref(),
            request_ref,
            response_snapshot_ref: requested_snapshot_ref,
            assessment_spec_ref,
            instrument_version_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            requested_output_schema_version: input.requested_output_schema_version,
        })
    }

    /// Reconstruct one version-pinned scoring request from durable stored identity.
    ///
    /// Call this after process restart when a scoring job still names
    /// `scoring_request_ref`. Do not use this to create a new dispatch identity;
    /// persist-time construction must stay on [`Self::from_snapshot`] so empty,
    /// unbound, or mismatched snapshots fail closed. The stored session and
    /// snapshot identities are accepted as pins; this does not reload response
    /// events, does not prove session/snapshot consistency, and does not call
    /// `fast-mlsirm`. Empty-snapshot rejection remains the persist-time
    /// [`Self::from_snapshot`] check.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] when a required or
    /// supplied optional reference is blank, or
    /// [`ScoringContractError::UnsupportedOutputSchemaVersion`] when the stored
    /// schema major is not supported by this runtime.
    pub fn from_persisted(
        session_ref: &str,
        input: ScoringRequestInput<'_>,
    ) -> Result<Self, ScoringContractError> {
        let session_ref = required_reference(session_ref)?;
        let request_ref = required_reference(input.scoring_request_ref)?;
        let requested_snapshot_ref = required_reference(input.response_snapshot_ref)?;
        let assessment_spec_ref = required_reference(input.assessment_spec_ref)?;
        let instrument_version_ref = required_reference(input.instrument_version_ref)?;
        let scoring_version_ref = required_reference(input.scoring_version_ref)?;
        let calibration_reference = required_reference(input.calibration_reference)?;
        let norm_version_ref = input.norm_version_ref.map(required_reference).transpose()?;
        Self::from_validated_pins(ValidatedScoringPins {
            session_ref,
            request_ref,
            response_snapshot_ref: requested_snapshot_ref,
            assessment_spec_ref,
            instrument_version_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            requested_output_schema_version: input.requested_output_schema_version,
        })
    }

    fn from_validated_pins(pins: ValidatedScoringPins<'_>) -> Result<Self, ScoringContractError> {
        if pins.requested_output_schema_version != SUPPORTED_OUTPUT_SCHEMA_VERSION {
            return Err(ScoringContractError::UnsupportedOutputSchemaVersion);
        }

        Ok(Self {
            request_ref: pins.request_ref.to_owned(),
            session_ref: pins.session_ref.to_owned(),
            response_snapshot_ref: pins.response_snapshot_ref.to_owned(),
            assessment_spec_ref: pins.assessment_spec_ref.to_owned(),
            instrument_version_ref: pins.instrument_version_ref.to_owned(),
            scoring_version_ref: pins.scoring_version_ref.to_owned(),
            calibration_reference: pins.calibration_reference.to_owned(),
            norm_version_ref: pins.norm_version_ref.map(str::to_owned),
            requested_output_schema_version: pins.requested_output_schema_version,
        })
    }

    /// Return the opaque idempotent scoring-request reference.
    #[must_use]
    pub fn scoring_request_ref(&self) -> &str {
        &self.request_ref
    }

    /// Return the session whose completed response snapshot is being scored.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the durable response-snapshot reference.
    #[must_use]
    pub fn response_snapshot_ref(&self) -> &str {
        &self.response_snapshot_ref
    }

    /// Return the exact assessment-contract reference.
    #[must_use]
    pub fn assessment_spec_ref(&self) -> &str {
        &self.assessment_spec_ref
    }

    /// Return the exact instrument-version reference.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the exact scoring-version reference.
    #[must_use]
    pub fn scoring_version_ref(&self) -> &str {
        &self.scoring_version_ref
    }

    /// Return the exact calibration artifact reference.
    #[must_use]
    pub fn calibration_reference(&self) -> &str {
        &self.calibration_reference
    }

    /// Return the optional norm-version reference.
    #[must_use]
    pub fn norm_version_ref(&self) -> Option<&str> {
        self.norm_version_ref.as_deref()
    }

    /// Return the requested scoring-output schema version.
    #[must_use]
    pub const fn requested_output_schema_version(&self) -> u16 {
        self.requested_output_schema_version
    }
}

/// Explicit disposition of one construct-level scoring observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservationDisposition {
    /// A finite numeric score was produced.
    Scored,
    /// The scoring contract intentionally declined to produce a score.
    Abstained,
    /// The observation failed without a valid numeric score.
    Failed,
    /// The observation was excluded by an explicit validity or policy rule.
    Excluded,
}

/// One immutable construct-level score observation.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreObservation {
    construct_ref: String,
    disposition: ObservationDisposition,
    score: Option<f64>,
    standard_error: Option<f64>,
}

impl ScoreObservation {
    /// Create a finite numeric scored observation.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] for a blank construct,
    /// [`ScoringContractError::InvalidScore`] for a non-finite score, or
    /// [`ScoringContractError::InvalidStandardError`] for a supplied standard
    /// error that is negative or non-finite.
    pub fn scored(
        construct_ref: impl Into<String>,
        score: f64,
        standard_error: Option<f64>,
    ) -> Result<Self, ScoringContractError> {
        let construct_ref = construct_ref.into();
        Self::scored_from_reference(&construct_ref, score, standard_error)
    }

    fn scored_from_reference(
        construct_ref: &str,
        score: f64,
        standard_error: Option<f64>,
    ) -> Result<Self, ScoringContractError> {
        let construct_ref = required_reference(construct_ref)?;
        if !score.is_finite() {
            return Err(ScoringContractError::InvalidScore);
        }
        if let Some(value) = standard_error {
            if !value.is_finite() || value < 0.0 {
                return Err(ScoringContractError::InvalidStandardError);
            }
        }
        Ok(Self {
            construct_ref: construct_ref.to_owned(),
            disposition: ObservationDisposition::Scored,
            score: Some(score),
            standard_error,
        })
    }

    /// Create an abstained, failed, or excluded observation with no numeric score.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] for a blank construct or
    /// [`ScoringContractError::ScoredDispositionRequiresScore`] if `Scored` is
    /// supplied without a numeric score.
    pub fn without_score(
        construct_ref: impl Into<String>,
        disposition: ObservationDisposition,
    ) -> Result<Self, ScoringContractError> {
        let construct_ref = construct_ref.into();
        let construct_ref = required_reference(&construct_ref)?;
        if disposition == ObservationDisposition::Scored {
            return Err(ScoringContractError::ScoredDispositionRequiresScore);
        }
        Ok(Self {
            construct_ref: construct_ref.to_owned(),
            disposition,
            score: None,
            standard_error: None,
        })
    }

    /// Return the construct measured by this observation.
    #[must_use]
    pub fn construct_ref(&self) -> &str {
        &self.construct_ref
    }

    /// Return whether this observation was scored, abstained, failed, or excluded.
    #[must_use]
    pub const fn disposition(&self) -> ObservationDisposition {
        self.disposition
    }

    /// Return the numeric score only for a scored observation.
    #[must_use]
    pub const fn score(&self) -> Option<f64> {
        self.score
    }

    /// Return the optional finite non-negative standard error.
    #[must_use]
    pub const fn standard_error(&self) -> Option<f64> {
        self.standard_error
    }
}

/// Immutable accepted scoring result bound to one scoring request.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoringResult {
    result_ref: String,
    request: ScoringRequest,
    engine_artifact_digest: String,
    observations: Vec<ScoreObservation>,
}

impl ScoringResult {
    /// Create an immutable scoring result without recomputing product-side scores.
    ///
    /// The engine artifact is immutable provenance, not a display label. ADR-0010 requires
    /// published artifacts to be content-addressed by cryptographic digest, so this boundary
    /// accepts the canonical `sha256:` prefix followed by exactly 64 lowercase hexadecimal bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] for a blank result identity,
    /// [`ScoringContractError::InvalidEngineArtifactDigest`] when engine provenance is not a
    /// canonical SHA-256 digest, [`ScoringContractError::EmptyObservationSet`] when no construct
    /// observation exists, or [`ScoringContractError::DuplicateConstruct`] when a construct
    /// appears more than once after reference normalization.
    pub fn new(
        scoring_result_ref: impl Into<String>,
        request: &ScoringRequest,
        engine_artifact_digest: impl Into<String>,
        observations: Vec<ScoreObservation>,
    ) -> Result<Self, ScoringContractError> {
        let scoring_result_ref = scoring_result_ref.into();
        let scoring_result_ref = required_reference(&scoring_result_ref)?;
        let engine_artifact_digest = engine_artifact_digest.into();
        let engine_artifact_digest = required_sha256_digest(&engine_artifact_digest)?;
        if observations.is_empty() {
            return Err(ScoringContractError::EmptyObservationSet);
        }

        let mut constructs = HashSet::with_capacity(observations.len());
        if observations
            .iter()
            .any(|observation| !constructs.insert(observation.construct_ref()))
        {
            return Err(ScoringContractError::DuplicateConstruct);
        }

        Ok(Self {
            result_ref: scoring_result_ref.to_owned(),
            request: request.clone(),
            engine_artifact_digest: engine_artifact_digest.to_owned(),
            observations,
        })
    }

    /// Return the opaque scoring-result reference.
    #[must_use]
    pub fn scoring_result_ref(&self) -> &str {
        &self.result_ref
    }

    /// Return the scoring request that produced this result.
    #[must_use]
    pub fn scoring_request_ref(&self) -> &str {
        self.request.scoring_request_ref()
    }

    /// Return the response snapshot scored by the engine.
    #[must_use]
    pub fn response_snapshot_ref(&self) -> &str {
        self.request.response_snapshot_ref()
    }

    /// Return the exact scoring-engine artifact digest.
    #[must_use]
    pub fn engine_artifact_digest(&self) -> &str {
        &self.engine_artifact_digest
    }

    /// Return immutable construct-level score observations.
    #[must_use]
    pub fn observations(&self) -> &[ScoreObservation] {
        &self.observations
    }

    /// Return whether this result is bound to the complete supplied request.
    #[must_use]
    pub(crate) fn matches_request(&self, request: &ScoringRequest) -> bool {
        &self.request == request
    }
}

/// Fail-closed validation error at the scoring-dispatch boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringContractError {
    /// A required or supplied optional reference is blank.
    EmptyReference,
    /// The response snapshot has not been assigned durable identity.
    UnboundResponseSnapshot,
    /// The response snapshot contains no accepted response event.
    EmptyResponseSnapshot,
    /// The supplied snapshot reference does not identify the supplied snapshot.
    ResponseSnapshotMismatch,
    /// The requested output schema major is not supported by this runtime.
    UnsupportedOutputSchemaVersion,
    /// Engine provenance is not canonical lowercase SHA-256 evidence.
    InvalidEngineArtifactDigest,
    /// A numeric score is NaN or infinite.
    InvalidScore,
    /// A score standard error is negative, NaN, or infinite.
    InvalidStandardError,
    /// A `Scored` disposition was requested without a numeric score.
    ScoredDispositionRequiresScore,
    /// A scoring result has no construct observations.
    EmptyObservationSet,
    /// More than one observation targets the same construct reference.
    DuplicateConstruct,
}

impl Display for ScoringContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyReference => {
                formatter.write_str("scoring contract references must not be empty")
            }
            Self::UnboundResponseSnapshot => {
                formatter.write_str("scoring requires a durable response snapshot reference")
            }
            Self::EmptyResponseSnapshot => {
                formatter.write_str("scoring requires at least one response event")
            }
            Self::ResponseSnapshotMismatch => formatter
                .write_str("scoring response snapshot reference does not match supplied snapshot"),
            Self::UnsupportedOutputSchemaVersion => {
                formatter.write_str("requested scoring output schema version is unsupported")
            }
            Self::InvalidEngineArtifactDigest => formatter.write_str(
                "scoring engine artifact digest must be sha256: followed by 64 lowercase hexadecimal characters",
            ),
            Self::InvalidScore => formatter.write_str("score values must be finite"),
            Self::InvalidStandardError => {
                formatter.write_str("score standard errors must be finite and non-negative")
            }
            Self::ScoredDispositionRequiresScore => {
                formatter.write_str("scored observations require a numeric score")
            }
            Self::EmptyObservationSet => {
                formatter.write_str("scoring results must contain at least one observation")
            }
            Self::DuplicateConstruct => formatter
                .write_str("scoring results must not contain duplicate construct references"),
        }
    }
}

impl Error for ScoringContractError {}

#[derive(Clone, Copy)]
struct ValidatedScoringPins<'a> {
    session_ref: &'a str,
    request_ref: &'a str,
    response_snapshot_ref: &'a str,
    assessment_spec_ref: &'a str,
    instrument_version_ref: &'a str,
    scoring_version_ref: &'a str,
    calibration_reference: &'a str,
    norm_version_ref: Option<&'a str>,
    requested_output_schema_version: u16,
}

fn required_reference(reference: &str) -> Result<&str, ScoringContractError> {
    normalized_reference(reference).ok_or(ScoringContractError::EmptyReference)
}

fn required_sha256_digest(digest: &str) -> Result<&str, ScoringContractError> {
    let Some(hex) = digest.strip_prefix(SHA256_PREFIX) else {
        return Err(ScoringContractError::InvalidEngineArtifactDigest);
    };
    if hex.len() != SHA256_HEX_LENGTH
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ScoringContractError::InvalidEngineArtifactDigest);
    }
    Ok(digest)
}
