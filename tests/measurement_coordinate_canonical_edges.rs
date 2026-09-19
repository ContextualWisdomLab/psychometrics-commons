//! Edge contracts for the released measurement-coordinate wire identity.

use psychometrics_commons_runtime::measurement_coordinate::{
    MeasurementCoordinateProvenance, MeasurementCoordinateProvenanceError,
};

const CANONICAL: &str = concat!(
    "psychometrics-commons.measurement-coordinate-provenance.v1\n",
    "contract_version=1:1\n",
    "assessment_spec_ref=41:assessment_spec_measurement_coordinate_v1\n",
    "instrument_version_ref=36:instrument_measurement_coordinate_v1\n",
    "scoring_version_ref=33:scoring_measurement_coordinate_v1\n",
    "calibration_reference=37:calibration_measurement_coordinate_v1\n",
    "norm_version_ref=some:30:norm_measurement_coordinate_v1\n",
    "requested_output_schema_version=1:1\n",
    "engine_artifact_digest=71:sha256:2222222222222222222222222222222222222222222222222222222222222222\n",
    "construct_ref=19:construct_predictor\n",
);

fn decode_error(bytes: &[u8]) -> MeasurementCoordinateProvenanceError {
    MeasurementCoordinateProvenance::from_canonical_bytes(bytes).unwrap_err()
}

#[test]
fn canonical_fixture_and_explicit_none_norm_round_trip() {
    let decoded = MeasurementCoordinateProvenance::from_canonical_bytes(CANONICAL.as_bytes())
        .expect("canonical fixture");
    assert_eq!(decoded.canonical_bytes(), CANONICAL.as_bytes());

    let no_norm = CANONICAL.replace(
        "norm_version_ref=some:30:norm_measurement_coordinate_v1",
        "norm_version_ref=none",
    );
    let decoded_no_norm = MeasurementCoordinateProvenance::from_canonical_bytes(no_norm.as_bytes())
        .expect("canonical absent norm");
    assert_eq!(decoded_no_norm.norm_version_ref(), None);
    assert_eq!(decoded_no_norm.canonical_bytes(), no_norm.as_bytes());
}

#[test]
fn decoder_rejects_structural_encoding_failures_before_constructing_authority() {
    assert_eq!(
        decode_error(&[0xff]),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(CANONICAL.trim_end_matches('\n').as_bytes()),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replacen(
                    "psychometrics-commons.measurement-coordinate-provenance.v1",
                    "wrong-domain",
                    1,
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace(
                    "scoring_version_ref=33:scoring_measurement_coordinate_v1\n",
                    "",
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace("contract_version=1:1", "contract_version=1")
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace("contract_version=1:1", "contract_version=x:1")
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace("assessment_spec_ref=41:", "assessment_spec_ref=40:")
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
}

#[test]
fn decoder_rejects_optional_field_aliases_and_length_failures() {
    assert_eq!(
        decode_error(
            CANONICAL
                .replace(
                    "norm_version_ref=some:30:norm_measurement_coordinate_v1",
                    "norm_version_ref=maybe",
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace(
                    "norm_version_ref=some:30:norm_measurement_coordinate_v1",
                    "norm_version_ref=some:x:norm_measurement_coordinate_v1",
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace(
                    "norm_version_ref=some:30:norm_measurement_coordinate_v1",
                    "norm_version_ref=some:29:norm_measurement_coordinate_v1",
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding
    );
}

#[test]
fn decoder_distinguishes_unsupported_contract_and_schema_versions() {
    assert_eq!(
        decode_error(
            CANONICAL
                .replace("contract_version=1:1", "contract_version=1:2")
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::UnsupportedContractVersion
    );
    assert_eq!(
        decode_error(
            CANONICAL
                .replace(
                    "requested_output_schema_version=1:1",
                    "requested_output_schema_version=1:2",
                )
                .as_bytes(),
        ),
        MeasurementCoordinateProvenanceError::UnsupportedOutputSchemaVersion
    );
}

#[test]
fn every_public_decoder_error_has_operator_distinguishable_text() {
    let cases = [
        MeasurementCoordinateProvenanceError::InvalidConstructReference,
        MeasurementCoordinateProvenanceError::UnknownConstruct,
        MeasurementCoordinateProvenanceError::UnscoredConstruct,
        MeasurementCoordinateProvenanceError::InvalidCanonicalEncoding,
        MeasurementCoordinateProvenanceError::NonCanonicalEncoding,
        MeasurementCoordinateProvenanceError::UnsupportedContractVersion,
        MeasurementCoordinateProvenanceError::InvalidProvenanceReference,
        MeasurementCoordinateProvenanceError::UnsupportedOutputSchemaVersion,
        MeasurementCoordinateProvenanceError::InvalidEngineArtifactDigest,
    ];

    for error in cases {
        assert!(!error.to_string().trim().is_empty());
    }
}
