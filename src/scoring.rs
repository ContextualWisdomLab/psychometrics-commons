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

pub(crate) const SUPPORTED_OUTPUT_SCHEMA_VERSION: u16 = 1;
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
    /// Identity-bearing references must already use their exact opaque spelling.
    /// The caller-supplied snapshot reference must exactly match the durable
    /// reference embedded in `snapshot`; aliases are not trimmed into a match.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] when a required or
    /// supplied optional reference is blank, whitespace-padded, control-bearing,
    /// or numeric-like,
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
        if input.requested_output_schema_version != SUPPORTED_OUTPUT_SCHEMA_VERSION {
            return Err(ScoringContractError::UnsupportedOutputSchemaVersion);
        }

        Ok(Self {
            request_ref: request_ref.to_owned(),
            session_ref: snapshot.session_ref().to_owned(),
            response_snapshot_ref: snapshot_ref.to_owned(),
            assessment_spec_ref: assessment_spec_ref.to_owned(),
            instrument_version_ref: instrument_version_ref.to_owned(),
            scoring_version_ref: scoring_version_ref.to_owned(),
            calibration_reference: calibration_reference.to_owned(),
            norm_version_ref: norm_version_ref.map(str::to_owned),
            requested_output_schema_version: input.requested_output_schema_version,
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
    score: Option<f64>,
    standard_error: Option<f64>,
    disposition: ObservationDisposition,
}

impl ScoreObservation {
    /// Build a scored observation with an optional finite non-negative standard error.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] for an invalid construct
    /// reference, [`ScoringContractError::InvalidScore`] for a non-finite score,
    /// or [`ScoringContractError::InvalidStandardError`] for a non-finite or
    /// negative standard error.
    pub fn scored(
        construct_ref: &str,
        score: f64,
        standard_error: Option<f64>,
    ) -> Result<Self, ScoringContractError> {
        let construct_ref = required_reference(construct_ref)?;
        if !score.is_finite() {
            return Err(ScoringContractError::InvalidScore);
        }
        if standard_error.is_some_and(|value| !value.is_finite() || value < 0.0) {
            return Err(ScoringContractError::InvalidStandardError);
        }
        Ok(Self {
            construct_ref: construct_ref.to_owned(),
            score: Some(score),
            standard_error,
            disposition: ObservationDisposition::Scored,
        })
    }

    /// Build a non-scored observation with an explicit non-success disposition.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] for an invalid construct
    /// reference or [`ScoringContractError::InvalidDisposition`] when `Scored`
    /// is supplied without a numeric score.
    pub fn without_score(
        construct_ref: &str,
        disposition: ObservationDisposition,
    ) -> Result<Self, ScoringContractError> {
        let construct_ref = required_reference(construct_ref)?;
        if disposition == ObservationDisposition::Scored {
            return Err(ScoringContractError::InvalidDisposition);
        }
        Ok(Self {
            construct_ref: construct_ref.to_owned(),
            score: None,
            standard_error: None,
            disposition,
        })
    }

    /// Return the exact construct reference.
    #[must_use]
    pub fn construct_ref(&self) -> &str {
        &self.construct_ref
    }

    /// Return the numeric score when the observation is scored.
    #[must_use]
    pub const fn score(&self) -> Option<f64> {
        self.score
    }

    /// Return the optional standard error for a scored observation.
    #[must_use]
    pub const fn standard_error(&self) -> Option<f64> {
        self.standard_error
    }

    /// Return the explicit observation disposition.
    #[must_use]
    pub const fn disposition(&self) -> ObservationDisposition {
        self.disposition
    }
}

/// Immutable output from one scoring request.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoringResult {
    result_ref: String,
    request_ref: String,
    response_snapshot_ref: String,
    engine_artifact_digest: String,
    observations: Vec<ScoreObservation>,
}

impl ScoringResult {
    /// Build an immutable scoring result for one request.
    ///
    /// # Errors
    ///
    /// Returns [`ScoringContractError::EmptyReference`] when the result reference
    /// is invalid, [`ScoringContractError::InvalidEngineArtifactDigest`] when the
    /// engine artifact digest is not canonical `sha256:<64-lowerhex>`,
    /// [`ScoringContractError::EmptyScoreObservations`] when no construct-level
    /// observations are present, or
    /// [`ScoringContractError::DuplicateConstructObservation`] when one construct
    /// appears more than once.
    pub fn new(
        result_ref: &str,
        request: &ScoringRequest,
        engine_artifact_digest: &str,
        observations: Vec<ScoreObservation>,
    ) -> Result<Self, ScoringContractError> {
        let result_ref = required_reference(result_ref)?;
        let engine_artifact_digest = required_sha256_digest(engine_artifact_digest)?;
        if observations.is_empty() {
            return Err(ScoringContractError::EmptyScoreObservations);
        }
        let mut constructs = HashSet::with_capacity(observations.len());
        for observation in &observations {
            if !constructs.insert(observation.construct_ref()) {
                return Err(ScoringContractError::DuplicateConstructObservation);
            }
        }
        Ok(Self {
            result_ref: result_ref.to_owned(),
            request_ref: request.scoring_request_ref().to_owned(),
            response_snapshot_ref: request.response_snapshot_ref().to_owned(),
            engine_artifact_digest: engine_artifact_digest.to_owned(),
            observations,
        })
    }

    /// Return the immutable result reference.
    #[must_use]
    pub fn result_ref(&self) -> &str {
        &self.result_ref
    }

    /// Return the scoring request reference bound to the result.
    #[must_use]
    pub fn scoring_request_ref(&self) -> &str {
        &self.request_ref
    }

    /// Return the response snapshot reference bound to the result.
    #[must_use]
    pub fn response_snapshot_ref(&self) -> &str {
        &self.response_snapshot_ref
    }

    /// Return the canonical scoring-engine artifact digest.
    #[must_use]
    pub fn engine_artifact_digest(&self) -> &str {
        &self.engine_artifact_digest
    }

    /// Return all construct-level score observations in contract order.
    #[must_use]
    pub fn score_observations(&self) -> &[ScoreObservation] {
        &self.observations
    }
}

/// Scoring-contract validation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoringContractError {
    /// A required identity-bearing reference was absent or invalid.
    EmptyReference,
    /// The supplied response snapshot has not been assigned a durable identity.
    UnboundResponseSnapshot,
    /// The supplied response snapshot has no response events to score.
    EmptyResponseSnapshot,
    /// The supplied response-snapshot reference does not match the snapshot.
    ResponseSnapshotMismatch,
    /// The requested output schema is not supported by this runtime.
    UnsupportedOutputSchemaVersion,
    /// A scored observation contains a non-finite numeric score.
    InvalidScore,
    /// A scored observation contains a non-finite or negative standard error.
    InvalidStandardError,
    /// `Scored` was used for an observation without a numeric score.
    InvalidDisposition,
    /// The scoring result contains no construct-level observations.
    EmptyScoreObservations,
    /// The scoring result repeats a construct reference.
    DuplicateConstructObservation,
    /// The engine artifact digest is not canonical `sha256:<64-lowerhex>`.
    InvalidEngineArtifactDigest,
}

impl Display for ScoringContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReference => "required scoring reference is absent or invalid",
            Self::UnboundResponseSnapshot => "response snapshot is not durably bound",
            Self::EmptyResponseSnapshot => "response snapshot has no scorable response events",
            Self::ResponseSnapshotMismatch => "scoring request response-snapshot reference does not match the durable snapshot",
            Self::UnsupportedOutputSchemaVersion => "requested scoring output schema is unsupported",
            Self::InvalidScore => "scored observation must contain a finite score",
            Self::InvalidStandardError => "scored observation standard error must be finite and non-negative",
            Self::InvalidDisposition => "scored disposition requires a numeric score",
            Self::EmptyScoreObservations => "scoring result must contain at least one construct observation",
            Self::DuplicateConstructObservation => "scoring result contains duplicate construct observations",
            Self::InvalidEngineArtifactDigest => "scoring engine artifact digest must be canonical sha256 lower-hex",
        })
    }
}

impl Error for ScoringContractError {}

fn required_reference(reference: &str) -> Result<&str, ScoringContractError> {
    let validated = normalized_reference(reference).ok_or(ScoringContractError::EmptyReference)?;
    if validated == reference {
        Ok(validated)
    } else {
        Err(ScoringContractError::EmptyReference)
    }
}

pub(crate) fn required_sha256_digest(digest: &str) -> Result<&str, ScoringContractError> {
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