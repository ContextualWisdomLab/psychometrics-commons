//! Result provenance preserves the intentional absence of a norm version.

#[path = "common/mod.rs"]
mod common;

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{ResultSnapshot, ResultSnapshotInput};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

#[test]
fn result_snapshot_preserves_absent_norm_without_inventing_provenance() {
    let response_snapshot = frozen_snapshot(
        "session_ref",
        "response_snapshot_ref",
        &[ResponseWrite {
            server_event_ref: "event_ref",
            client_event_ref: "client_ref",
            item_version_ref: "item_version_ref",
            payload_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    let request = ScoringRequest::from_snapshot(
        &response_snapshot,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_ref",
            response_snapshot_ref: "response_snapshot_ref",
            assessment_spec_ref: "assessment_spec_ref",
            instrument_version_ref: "instrument_version_ref",
            scoring_version_ref: "scoring_version_ref",
            calibration_reference: "calibration_reference",
            norm_version_ref: None,
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let result = ScoringResult::new(
        "scoring_result_ref",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_ref", 1.0, None).unwrap()],
    )
    .unwrap();
    let session = common::scoring_session(
        request.session_ref(),
        "participant_ref",
        request.instrument_version_ref(),
    );
    let snapshot = ResultSnapshot::new(
        &session,
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["service_consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
    )
    .unwrap();

    assert_eq!(snapshot.norm_version_ref(), None);
}
