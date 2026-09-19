//! Participant-free provenance for one scored measurement coordinate.
//!
//! Analytical consumers need to distinguish coordinates produced under different
//! measurement definitions without importing product participant state or
//! psychometric arithmetic. This projection is traceability evidence; it does
//! not establish construct validity, invariance, calibration adequacy, fairness,
//! linking, or fitness for a downstream intended use.

use crate::reference::normalized_reference;
use crate::result::ResultSnapshot;
use crate::scoring::ObservationDisposition;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Semantic version of the measurement-coordinate provenance contract.
pub const MEASUREMENT_COORDINATE_CONTRACT_VERSION: u16 = 1;

const CANONICAL_DOMAIN: &str = "psychometrics-commons.measurement-coordinate-provenance.v1";

/// Immutable, participant-free authority for one scored construct coordinate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementCoordinateProvenance {
    assessment_spec_ref: String,
    instrument_version_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    requested_output_schema_version: u16,
    engine_artifact_digest: String,
    construct_ref: String,
}

impl MeasurementCoordinateProvenance {
    /// Project one scored construct from an immutable result snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the construct reference is not exact, absent,
    /// or not a scored observation.
    pub fn from_result_snapshot(
        snapshot: &ResultSnapshot,
        construct_ref: &str,
    ) -> Result<Self, MeasurementCoordinateProvenanceError> {
        let validated_construct_ref = normalized_reference(construct_ref)
            .filter(|validated| *validated == construct_ref)
            .ok_or(MeasurementCoordinateProvenanceError::InvalidConstructReference)?;
        let observation = snapshot
            .score_observations()
            .iter()
            .find(|observation| observation.construct_ref() == validated_construct_ref)
            .ok_or(MeasurementCoordinateProvenanceError::UnknownConstruct)?;
        if observation.disposition() != ObservationDisposition::Scored {
            return Err(MeasurementCoordinateProvenanceError::UnscoredConstruct);
        }

        Ok(Self {
            assessment_spec_ref: snapshot.assessment_spec_ref().to_owned(),
            instrument_version_ref: snapshot.instrument_version_ref().to_owned(),
            scoring_version_ref: snapshot.scoring_version_ref().to_owned(),
            calibration_reference: snapshot.calibration_reference().to_owned(),
            norm_version_ref: snapshot.norm_version_ref().map(str::to_owned),
            requested_output_schema_version: snapshot.requested_output_schema_version(),
            engine_artifact_digest: snapshot.engine_artifact_digest().to_owned(),
            construct_ref: validated_construct_ref.to_owned(),
        })
    }

    /// Return the semantic contract version.
    #[must_use]
    pub const fn contract_version(&self) -> u16 {
        MEASUREMENT_COORDINATE_CONTRACT_VERSION
    }

    /// Return the exact assessment specification reference.
    #[must_use]
    pub fn assessment_spec_ref(&self) -> &str {
        &self.assessment_spec_ref
    }

    /// Return the exact published instrument-version reference.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the exact scoring-policy or scoring-engine contract version.
    #[must_use]
    pub fn scoring_version_ref(&self) -> &str {
        &self.scoring_version_ref
    }

    /// Return the exact calibration artifact reference.
    #[must_use]
    pub fn calibration_reference(&self) -> &str {
        &self.calibration_reference
    }

    /// Return the optional exact norm-version reference.
    #[must_use]
    pub fn norm_version_ref(&self) -> Option<&str> {
        self.norm_version_ref.as_deref()
    }

    /// Return the scoring-output schema version pinned by the scoring request.
    #[must_use]
    pub const fn requested_output_schema_version(&self) -> u16 {
        self.requested_output_schema_version
    }

    /// Return the canonical scoring-engine artifact digest.
    #[must_use]
    pub fn engine_artifact_digest(&self) -> &str {
        &self.engine_artifact_digest
    }

    /// Return the exact construct represented by this coordinate authority.
    #[must_use]
    pub fn construct_ref(&self) -> &str {
        &self.construct_ref
    }

    /// Encode the provenance into deterministic, length-delimited bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(512);
        encoded.extend_from_slice(CANONICAL_DOMAIN.as_bytes());
        encoded.push(b'\n');
        append_field(&mut encoded, "contract_version", &MEASUREMENT_COORDINATE_CONTRACT_VERSION.to_string());
        append_field(&mut encoded, "assessment_spec_ref", &self.assessment_spec_ref);
        append_field(&mut encoded, "instrument_version_ref", &self.instrument_version_ref);
        append_field(&mut encoded, "scoring_version_ref", &self.scoring_version_ref);
        append_field(&mut encoded, "calibration_reference", &self.calibration_reference);
        append_optional_field(&mut encoded, "norm_version_ref", self.norm_version_ref.as_deref());
        append_field(&mut encoded, "requested_output_schema_version", &self.requested_output_schema_version.to_string());
        append_field(&mut encoded, "engine_artifact_digest", &self.engine_artifact_digest);
        append_field(&mut encoded, "construct_ref", &self.construct_ref);
        encoded
    }
}

/// Fail-closed errors for measurement-coordinate provenance projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MeasurementCoordinateProvenanceError {
    /// The requested construct reference is not an exact safe opaque identity.
    InvalidConstructReference,
    /// The immutable result snapshot has no observation for the requested construct.
    UnknownConstruct,
    /// The requested construct exists but has no scored numeric observation.
    UnscoredConstruct,
}

impl Display for MeasurementCoordinateProvenanceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConstructReference => "measurement-coordinate construct reference must use its exact safe opaque spelling",
            Self::UnknownConstruct => "measurement-coordinate construct is absent from the immutable result snapshot",
            Self::UnscoredConstruct => "measurement-coordinate provenance requires a scored construct observation",
        })
    }
}

impl Error for MeasurementCoordinateProvenanceError {}

fn append_field(target: &mut Vec<u8>, name: &str, value: &str) {
    target.extend_from_slice(name.as_bytes());
    target.push(b'=');
    target.extend_from_slice(value.len().to_string().as_bytes());
    target.push(b':');
    target.extend_from_slice(value.as_bytes());
    target.push(b'\n');
}

fn append_optional_field(target: &mut Vec<u8>, name: &str, value: Option<&str>) {
    target.extend_from_slice(name.as_bytes());
    target.push(b'=');
    match value {
        Some(value) => {
            target.extend_from_slice(b"some:");
            target.extend_from_slice(value.len().to_string().as_bytes());
            target.push(b':');
            target.extend_from_slice(value.as_bytes());
        }
        None => target.extend_from_slice(b"none"),
    }
    target.push(b'\n');
}
