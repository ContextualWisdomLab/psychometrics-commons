//! Exact-spelling regressions for scoring identity and provenance references.
//!
//! Scoring references are opaque issued identifiers or exact version/provenance
//! references. Leading or trailing whitespace must not be silently removed,
//! because that would accept a spelling the caller did not actually present.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use response_support::{active_session, completed_session};

const ENGINE_DIGEST: &str =
    "sha256:7777777777777777777777777777777777777777777777777777777777777777";

fn completed_snapshot() -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let active = active_session("session_scoring_exact_ref");
    let mut ledger = ResponseLedger::from_session(&active).unwrap();
    ledger
        .record(
            &active,
            ResponseWrite {
                server_event_ref: "event_scoring_exact_ref",
                client_event_ref: "client_scoring_exact_ref",
                item_version_ref: "item_scoring_exact_ref",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let completed = completed_session("session_scoring_exact_ref");
    ledger
        .freeze_as(&completed, "snapshot_scoring_exact_ref")
        .unwrap()
}

fn scoring_input() -> ScoringRequestInput<'static> {
    ScoringRequestInput {
        scoring_request_ref: "request_scoring_exact_ref",
        response_snapshot_ref: "snapshot_scoring_exact_ref",
        assessment_spec_ref: "assessment_spec_exact_ref",
        instrument_version_ref: "instrument_version_exact_ref",
        scoring_version_ref: "scoring_version_exact_ref",
        calibration_reference: "calibration_exact_ref",
        norm_version_ref: Some("norm_version_exact_ref"),
        requested_output_schema_version: 1,
    }
}

fn scoring_request() -> psychometrics_commons_runtime::scoring::ScoringRequest {
    ScoringRequest::from_snapshot(&completed_snapshot(), scoring_input()).unwrap()
}

#[test]
fn scoring_request_rejects_padded_identity_and_provenance_aliases() {
    let snapshot = completed_snapshot();

    let mut input = scoring_input();
    input.scoring_request_ref = " request_scoring_exact_ref ";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.response_snapshot_ref = "\u{00a0}snapshot_scoring_exact_ref\u{00a0}";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.assessment_spec_ref = "\u{2003}assessment_spec_exact_ref\u{2003}";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.instrument_version_ref = "\u{202f}instrument_version_exact_ref\u{202f}";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.scoring_version_ref = " scoring_version_exact_ref ";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.calibration_reference = " calibration_exact_ref ";
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());

    let mut input = scoring_input();
    input.norm_version_ref = Some(" norm_version_exact_ref ");
    assert!(ScoringRequest::from_snapshot(&snapshot, input).is_err());
}

#[test]
fn observations_and_results_reject_padded_aliases_without_changing_exact_values() {
    assert!(ScoreObservation::scored(" construct_extraversion ", 0.75, Some(0.10)).is_err());
    assert!(ScoreObservation::without_score(
        "\u{3000}construct_neuroticism\u{3000}",
        ObservationDisposition::Abstained,
    )
    .is_err());

    let request = scoring_request();
    let exact = ScoreObservation::scored("구성개념_개방성", 0.25, Some(0.05)).unwrap();
    assert_eq!(exact.construct_ref(), "구성개념_개방성");

    assert!(ScoringResult::new(
        " result_scoring_exact_ref ",
        &request,
        ENGINE_DIGEST,
        vec![exact.clone()],
    )
    .is_err());

    let result = ScoringResult::new(
        "결과_scoring_exact_ref",
        &request,
        ENGINE_DIGEST,
        vec![exact],
    )
    .unwrap();
    assert_eq!(result.scoring_result_ref(), "결과_scoring_exact_ref");
    assert_eq!(result.observations()[0].construct_ref(), "구성개념_개방성");
}
