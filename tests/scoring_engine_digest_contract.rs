//! Regression contract for immutable scoring-engine artifact identity.
//!
//! ADR-0010 requires published artifacts to be content-addressed by cryptographic digest and
//! treats a digest mismatch as fatal. The product scoring boundary therefore accepts only a
//! canonical lowercase SHA-256 identity for the engine artifact instead of allowing a placeholder
//! or human-readable token to become immutable result provenance.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringContractError, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const CANONICAL_ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn scoring_request() -> ScoringRequest {
    let mut ledger = ResponseLedger::new("session_engine_digest_contract").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_engine_digest_contract",
                client_event_ref: "client_event_engine_digest_contract",
                item_version_ref: "item_version_engine_digest_contract",
                payload_digest: "sha256:response_engine_digest_contract",
            },
        )
        .unwrap();
    let snapshot = ledger
        .freeze_as(
            SessionState::Completed,
            "response_snapshot_engine_digest_contract",
        )
        .unwrap();

    ScoringRequest::from_snapshot(
        &snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_engine_digest_contract",
            response_snapshot_ref: "response_snapshot_engine_digest_contract",
            assessment_spec_ref: "assessment_spec_engine_digest_contract",
            instrument_version_ref: "instrument_version_engine_digest_contract",
            scoring_version_ref: "scoring_version_engine_digest_contract",
            calibration_reference: "calibration_engine_digest_contract",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn observation() -> ScoreObservation {
    ScoreObservation::scored("big_five_openness", 0.25, Some(0.10)).unwrap()
}

#[test]
fn scoring_result_accepts_canonical_sha256_engine_artifact_digest() {
    let request = scoring_request();
    let result = ScoringResult::new(
        "scoring_result_engine_digest_contract",
        &request,
        CANONICAL_ENGINE_DIGEST,
        vec![observation()],
    )
    .unwrap();

    assert_eq!(result.engine_artifact_digest(), CANONICAL_ENGINE_DIGEST);
}

#[test]
fn scoring_result_rejects_noncanonical_engine_artifact_digest() {
    let request = scoring_request();
    for invalid_digest in [
        "sha256:engine",
        "sha256:000000000000000000000000000000000000000000000000000000000000000",
        "sha256:000000000000000000000000000000000000000000000000000000000000000G",
        "SHA256:0000000000000000000000000000000000000000000000000000000000000000",
        "md5:00000000000000000000000000000000",
    ] {
        let error = ScoringResult::new(
            "scoring_result_engine_digest_contract",
            &request,
            invalid_digest,
            vec![observation()],
        )
        .err()
        .expect("noncanonical engine artifact digest must fail closed");

        assert_eq!(error, ScoringContractError::InvalidEngineArtifactDigest);
        assert_eq!(
            error.to_string(),
            "scoring engine artifact digest must be sha256: followed by 64 lowercase hexadecimal characters"
        );
    }
}
