//! Coverage and fail-closed regressions for required immutable result references.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::ResponseWrite;
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::frozen_snapshot;

const ENGINE_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn request_and_result() -> (ScoringRequest, ScoringResult) {
    let snapshot = frozen_snapshot(
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
        &snapshot,
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
        vec![ScoreObservation::scored("construct_ref", 0.0, None).unwrap()],
    )
    .unwrap();
    (request, result)
}

fn result_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_snapshot_ref",
        participant_ref: "participant_ref",
        narrative_version_ref: "narrative_version_ref",
        consent_snapshot_refs: &["service_consent_ref"],
        created_at_unix_ms: 1,
        supersedes_ref: None,
    }
}

#[test]
fn result_snapshot_rejects_blank_snapshot_identity() {
    let (request, result) = request_and_result();
    let mut input = result_input();
    input.result_snapshot_ref = "   ";

    assert_eq!(
        ResultSnapshot::new(&request, &result, input).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );
}

#[test]
fn result_snapshot_rejects_blank_narrative_version() {
    let (request, result) = request_and_result();
    let mut input = result_input();
    input.narrative_version_ref = "\t";

    assert_eq!(
        ResultSnapshot::new(&request, &result, input).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );
}

#[test]
fn result_snapshot_rejects_edge_whitespace_instead_of_normalizing_identity() {
    let (request, result) = request_and_result();

    for (field, alias) in [
        ("result_snapshot_ref", "  result_snapshot_ref  "),
        ("participant_ref", "\u{00a0}participant_ref\u{00a0}"),
        (
            "narrative_version_ref",
            "\u{2003}narrative_version_ref\u{2003}",
        ),
    ] {
        let mut input = result_input();
        match field {
            "result_snapshot_ref" => input.result_snapshot_ref = alias,
            "participant_ref" => input.participant_ref = alias,
            _ => input.narrative_version_ref = alias,
        }
        assert_eq!(
            ResultSnapshot::new(&request, &result, input),
            Err(ResultSnapshotError::EmptyReference),
            "{field} must reject non-canonical spelling instead of trimming into another identity",
        );
    }

    let mut input = result_input();
    input.consent_snapshot_refs = &["\u{00a0}service_consent_ref\u{00a0}"];
    assert_eq!(
        ResultSnapshot::new(&request, &result, input),
        Err(ResultSnapshotError::EmptyReference),
        "consent snapshot references must reject non-canonical spelling",
    );
}
