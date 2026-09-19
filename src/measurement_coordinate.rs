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
const SUPPORTED_OUTPUT_SCHEMA_VERSION: u16 = 1;
const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LENGTH: usize = 64;

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
        let validated_construct_ref = exact_reference(construct_ref)
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

    /// Decode one canonical released measurement-coordinate contract.
    ///
    /// The decoder is intentionally strict: it accepts one field order and one
    /// spelling for lengths, option presence, supported schema version, digest,
    /// and opaque references. This prevents aliases from becoming distinct
    /// cross-repository identities for the same authority.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed or non-canonical bytes, unsupported
    /// contract/schema versions, invalid provenance references, or a malformed
    /// scoring-engine artifact digest.
    pub fn from_canonical_bytes(
        bytes: &[u8],
    ) -> Result<Self, MeasurementCoordinateProvenanceError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
        if !text.ends_with('\n') {
            return Err(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding);
        }

        let mut lines = text.split_terminator('\n');
        if lines.next() != Some(CANONICAL_DOMAIN) {
            return Err(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding);
        }

        let contract_version = parse_field(&mut lines, "contract_version")?
            .parse::<u16>()
            .map_err(|_| MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
        if contract_version != MEASUREMENT_COORDINATE_CONTRACT_VERSION {
            return Err(MeasurementCoordinateProvenanceError::UnsupportedContractVersion);
        }

        let assessment_spec_ref = validated_owned_reference(parse_field(
            &mut lines,
            "assessment_spec_ref",
        )?)?;
        let instrument_version_ref = validated_owned_reference(parse_field(
            &mut lines,
            "instrument_version_ref",
        )?)?;
        let scoring_version_ref = validated_owned_reference(parse_field(
            &mut lines,
            "scoring_version_ref",
        )?)?;
        let calibration_reference = validated_owned_reference(parse_field(
            &mut lines,
            "calibration_reference",
        )?)?;
        let norm_version_ref = parse_optional_field(&mut lines, "norm_version_ref")?
            .map(validated_owned_reference)
            .transpose()?;

        let requested_output_schema_version = parse_field(
            &mut lines,
            "requested_output_schema_version",
        )?
        .parse::<u16>()
        .map_err(|_| MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
        if requested_output_schema_version != SUPPORTED_OUTPUT_SCHEMA_VERSION {
            return Err(MeasurementCoordinateProvenanceError::UnsupportedOutputSchemaVersion);
        }

        let engine_artifact_digest = parse_field(&mut lines, "engine_artifact_digest")?;
        if !is_canonical_sha256_digest(engine_artifact_digest) {
            return Err(MeasurementCoordinateProvenanceError::InvalidEngineArtifactDigest);
        }
        let construct_ref = validated_owned_reference(parse_field(&mut lines, "construct_ref")?)?;

        if lines.next().is_some() {
            return Err(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding);
        }

        let decoded = Self {
            assessment_spec_ref,
            instrument_version_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            requested_output_schema_version,
            engine_artifact_digest: engine_artifact_digest.to_owned(),
            construct_ref,
        };
        if decoded.canonical_bytes() != bytes {
            return Err(MeasurementCoordinateProvenanceError::NonCanonicalEncoding);
        }
        Ok(decoded)
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
        append_field(
            &mut encoded,
            "contract_version",
            &MEASUREMENT_COORDINATE_CONTRACT_VERSION.to_string(),
        );
        append_field(&mut encoded, "assessment_spec_ref", &self.assessment_spec_ref);
        append_field(
            &mut encoded,
            "instrument_version_ref",
            &self.instrument_version_ref,
        );
        append_field(&mut encoded, "scoring_version_ref", &self.scoring_version_ref);
        append_field(
            &mut encoded,
            "calibration_reference",
            &self.calibration_reference,
        );
        append_optional_field(&mut encoded, "norm_version_ref", self.norm_version_ref.as_deref());
        append_field(
            &mut encoded,
            "requested_output_schema_version",
            &self.requested_output_schema_version.to_string(),
        );
        append_field(
            &mut encoded,
            "engine_artifact_digest",
            &self.engine_artifact_digest,
        );
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
    /// The canonical byte contract is malformed or contains unexpected fields.
    InvalidCanonicalEncoding,
    /// The byte contract is semantically valid but not in its unique canonical spelling.
    NonCanonicalEncoding,
    /// The byte contract names a measurement-coordinate contract version this runtime cannot consume.
    UnsupportedContractVersion,
    /// A decoded measurement/scoring/construct reference is not an exact safe opaque identity.
    InvalidProvenanceReference,
    /// The byte contract names a scoring-output schema this runtime cannot consume.
    UnsupportedOutputSchemaVersion,
    /// The decoded engine artifact digest is not canonical `sha256:<64-lowerhex>`.
    InvalidEngineArtifactDigest,
}

impl Display for MeasurementCoordinateProvenanceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConstructReference => {
                "measurement-coordinate construct reference must use its exact safe opaque spelling"
            }
            Self::UnknownConstruct => {
                "measurement-coordinate construct is absent from the immutable result snapshot"
            }
            Self::UnscoredConstruct => {
                "measurement-coordinate provenance requires a scored construct observation"
            }
            Self::InvalidCanonicalEncoding => {
                "measurement-coordinate provenance bytes are malformed or contain unexpected fields"
            }
            Self::NonCanonicalEncoding => {
                "measurement-coordinate provenance bytes do not use the unique canonical spelling"
            }
            Self::UnsupportedContractVersion => {
                "measurement-coordinate provenance contract version is unsupported"
            }
            Self::InvalidProvenanceReference => {
                "measurement-coordinate provenance contains an invalid or aliased reference"
            }
            Self::UnsupportedOutputSchemaVersion => {
                "measurement-coordinate scoring output schema is unsupported"
            }
            Self::InvalidEngineArtifactDigest => {
                "measurement-coordinate engine artifact digest must be canonical sha256 lower-hex"
            }
        })
    }
}

impl Error for MeasurementCoordinateProvenanceError {}

fn exact_reference(reference: &str) -> Option<&str> {
    normalized_reference(reference).filter(|validated| *validated == reference)
}

fn validated_owned_reference(
    reference: &str,
) -> Result<String, MeasurementCoordinateProvenanceError> {
    exact_reference(reference)
        .map(str::to_owned)
        .ok_or(MeasurementCoordinateProvenanceError::InvalidProvenanceReference)
}

fn is_canonical_sha256_digest(digest: &str) -> bool {
    digest
        .strip_prefix(SHA256_PREFIX)
        .is_some_and(|hex| {
            hex.len() == SHA256_HEX_LENGTH
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn parse_field<'a, I>(
    lines: &mut I,
    expected_name: &str,
) -> Result<&'a str, MeasurementCoordinateProvenanceError>
where
    I: Iterator<Item = &'a str>,
{
    let line = lines
        .next()
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let value_with_length = line
        .strip_prefix(expected_name)
        .and_then(|remainder| remainder.strip_prefix('='))
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let (length_text, value) = value_with_length
        .split_once(':')
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let declared_length = length_text
        .parse::<usize>()
        .map_err(|_| MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    if declared_length != value.len() {
        return Err(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding);
    }
    Ok(value)
}

fn parse_optional_field<'a, I>(
    lines: &mut I,
    expected_name: &str,
) -> Result<Option<&'a str>, MeasurementCoordinateProvenanceError>
where
    I: Iterator<Item = &'a str>,
{
    let line = lines
        .next()
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let encoded = line
        .strip_prefix(expected_name)
        .and_then(|remainder| remainder.strip_prefix('='))
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    if encoded == "none" {
        return Ok(None);
    }
    let present = encoded
        .strip_prefix("some:")
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let (length_text, value) = present
        .split_once(':')
        .ok_or(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    let declared_length = length_text
        .parse::<usize>()
        .map_err(|_| MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding)?;
    if declared_length != value.len() {
        return Err(MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding);
    }
    Ok(Some(value))
}

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