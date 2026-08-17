//! Immutable product result snapshots and supersession provenance.
//!
//! A result snapshot copies the exact scientific provenance and score
//! observations returned by the scoring boundary. The product runtime may add
//! presentation and consent references, but it does not recompute psychometric
//! values or mutate a historical snapshot when norms or narratives change.

use crate::reference::canonical_opaque_reference;
use crate::scoring::{ScoreObservation, ScoringRequest, ScoringResult};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Borrowed product metadata needed to publish one immutable result snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultSnapshotInput<'a> {
    /// New opaque result-snapshot reference.
    pub result_snapshot_ref: &'a str,
    /// Product participant that owns the personal result.
    pub participant_ref: &'a str,
    /// Exact approved narrative-rule or deterministic-template version.
    pub narrative_version_ref: &'a str,
    /// Immutable consent snapshots authorizing the result processing context.
    pub consent_snapshot_refs: &'a [&'a str],
    /// Server-assigned creation time as Unix milliseconds.
    pub created_at_unix_ms: u64,
    /// Optional earlier result snapshot superseded by this new snapshot.
    pub supersedes_ref: Option<&'a str>,
}

/// Immutable product result with copied score observations and full provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultSnapshot {
    snapshot_ref: String,
    participant_ref: String,
    scoring_result_ref: String,
    session_ref: String,
    response_snapshot_ref: String,
    assessment_spec_ref: String,
    instrument_version_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    requested_output_schema_version: u16,
    narrative_version_ref: String,
    consent_snapshot_refs: Vec<String>,
    engine_artifact_digest: String,
    score_observations: Vec<ScoreObservation>,
    created_at_unix_ms: u64,
    supersedes_ref: Option<String>,
}

impl ResultSnapshot {
    /// Create a result snapshot by copying the scoring request and engine output.
    ///
    /// Product-owned references must already use their exact opaque spelling
    /// before they become identity-bearing state. Scientific provenance is copied
    /// verbatim from the already validated scoring request/result boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSnapshotError::ScoringRequestMismatch`] when `result`
    /// belongs to another scoring request, [`ResultSnapshotError::EmptyReference`]
    /// for any blank, whitespace-padded, control-bearing, or numeric-like
    /// required/supersession/consent reference,
    /// [`ResultSnapshotError::MissingConsentSnapshot`] when no consent evidence
    /// is supplied, [`ResultSnapshotError::DuplicateConsentSnapshot`] when the
    /// same consent reference appears more than once,
    /// [`ResultSnapshotError::InvalidCreationTime`] when creation time is zero,
    /// or [`ResultSnapshotError::SelfSupersession`] when a snapshot reference
    /// claims to supersede itself.
    pub fn new(
        request: &ScoringRequest,
        result: &ScoringResult,
        input: ResultSnapshotInput<'_>,
    ) -> Result<Self, ResultSnapshotError> {
        if !result.matches_request(request) {
            return Err(ResultSnapshotError::ScoringRequestMismatch);
        }

        let snapshot_ref = required_reference(input.result_snapshot_ref)?;
        let participant_ref = required_reference(input.participant_ref)?;
        let narrative_version_ref = required_reference(input.narrative_version_ref)?;
        if input.consent_snapshot_refs.is_empty() {
            return Err(ResultSnapshotError::MissingConsentSnapshot);
        }

        let mut consent_refs = HashSet::with_capacity(input.consent_snapshot_refs.len());
        let mut normalized_consents = Vec::with_capacity(input.consent_snapshot_refs.len());
        for consent_ref in input.consent_snapshot_refs {
            let consent_ref = required_reference(consent_ref)?;
            if !consent_refs.insert(consent_ref.to_owned()) {
                return Err(ResultSnapshotError::DuplicateConsentSnapshot);
            }
            normalized_consents.push(consent_ref.to_owned());
        }

        if input.created_at_unix_ms == 0 {
            return Err(ResultSnapshotError::InvalidCreationTime);
        }

        let supersedes_ref = input.supersedes_ref.map(required_reference).transpose()?;
        if supersedes_ref == Some(snapshot_ref) {
            return Err(ResultSnapshotError::SelfSupersession);
        }

        Ok(Self {
            snapshot_ref: snapshot_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            scoring_result_ref: result.scoring_result_ref().to_owned(),
            session_ref: request.session_ref().to_owned(),
            response_snapshot_ref: request.response_snapshot_ref().to_owned(),
            assessment_spec_ref: request.assessment_spec_ref().to_owned(),
            instrument_version_ref: request.instrument_version_ref().to_owned(),
            scoring_version_ref: request.scoring_version_ref().to_owned(),
            calibration_reference: request.calibration_reference().to_owned(),
            norm_version_ref: request.norm_version_ref().map(str::to_owned),
            requested_output_schema_version: request.requested_output_schema_version(),
            narrative_version_ref: narrative_version_ref.to_owned(),
            consent_snapshot_refs: normalized_consents,
            engine_artifact_digest: result.engine_artifact_digest().to_owned(),
            score_observations: result.observations().to_vec(),
            created_at_unix_ms: input.created_at_unix_ms,
            supersedes_ref: supersedes_ref.map(str::to_owned),
        })
    }

    /// Return this immutable result-snapshot reference.
    #[must_use]
    pub fn result_snapshot_ref(&self) -> &str {
        &self.snapshot_ref
    }

    /// Return the product participant that owns this personal result.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the scoring-engine result copied into this snapshot.
    #[must_use]
    pub fn scoring_result_ref(&self) -> &str {
        &self.scoring_result_ref
    }

    /// Return the assessment session that produced the scored response snapshot.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the exact response snapshot that was scored.
    #[must_use]
    pub fn response_snapshot_ref(&self) -> &str {
        &self.response_snapshot_ref
    }

    /// Return the exact assessment specification used for scoring.
    #[must_use]
    pub fn assessment_spec_ref(&self) -> &str {
        &self.assessment_spec_ref
    }

    /// Return the immutable published instrument version.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the exact scoring version.
    #[must_use]
    pub fn scoring_version_ref(&self) -> &str {
        &self.scoring_version_ref
    }

    /// Return the exact calibration artifact reference.
    #[must_use]
    pub fn calibration_reference(&self) -> &str {
        &self.calibration_reference
    }

    /// Return the optional norm version used by the scoring request.
    #[must_use]
    pub fn norm_version_ref(&self) -> Option<&str> {
        self.norm_version_ref.as_deref()
    }

    /// Return the exact scoring-output schema version pinned by the request.
    #[must_use]
    pub const fn requested_output_schema_version(&self) -> u16 {
        self.requested_output_schema_version
    }

    /// Return the independently versioned narrative rule/template reference.
    #[must_use]
    pub fn narrative_version_ref(&self) -> &str {
        &self.narrative_version_ref
    }

    /// Return immutable consent snapshots that govern this result context.
    #[must_use]
    pub fn consent_snapshot_refs(&self) -> &[String] {
        &self.consent_snapshot_refs
    }

    /// Return the exact scoring-engine artifact digest.
    #[must_use]
    pub fn engine_artifact_digest(&self) -> &str {
        &self.engine_artifact_digest
    }

    /// Return copied construct-level score observations without recomputation.
    #[must_use]
    pub fn score_observations(&self) -> &[ScoreObservation] {
        &self.score_observations
    }

    /// Return the server-assigned creation time as Unix milliseconds.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return the earlier result snapshot superseded by this snapshot, if any.
    #[must_use]
    pub fn supersedes_ref(&self) -> Option<&str> {
        self.supersedes_ref.as_deref()
    }
}

/// Fail-closed validation error for immutable product result snapshots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResultSnapshotError {
    /// A required or supplied optional reference is blank, noncanonical, or numeric-like.
    EmptyReference,
    /// Result publication was attempted without any consent snapshot evidence.
    MissingConsentSnapshot,
    /// The same consent snapshot reference appears more than once.
    DuplicateConsentSnapshot,
    /// Server creation time zero is not valid publication evidence.
    InvalidCreationTime,
    /// A result snapshot names itself as the result it supersedes.
    SelfSupersession,
    /// The scoring result belongs to a different scoring request.
    ScoringRequestMismatch,
}

impl Display for ResultSnapshotError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyReference => {
                formatter.write_str("result snapshot references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters")
            }
            Self::MissingConsentSnapshot => formatter
                .write_str("result snapshots require at least one consent snapshot reference"),
            Self::DuplicateConsentSnapshot => formatter
                .write_str("result snapshots must not contain duplicate consent references"),
            Self::InvalidCreationTime => {
                formatter.write_str("result snapshot creation time must be positive")
            }
            Self::SelfSupersession => {
                formatter.write_str("a result snapshot cannot supersede itself")
            }
            Self::ScoringRequestMismatch => formatter
                .write_str("scoring result does not belong to the supplied scoring request"),
        }
    }
}

impl Error for ResultSnapshotError {}

fn required_reference(reference: &str) -> Result<&str, ResultSnapshotError> {
    canonical_opaque_reference(reference).ok_or(ResultSnapshotError::EmptyReference)
}
