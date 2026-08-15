//! Fail-first regressions for scoring provenance binding and reference validation.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringContractError, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

fn ledger_with_one_response() -> ResponseLedger {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "event_ref",
                client_event_ref: "client_ref",
                item_version_ref: "item_version_ref",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    ledger
}

fn scoring_input(response_snapshot_ref: &str) -> ScoringRequestInput<'_> {
    ScoringRequestInput {
        scoring_request_ref: "scoring_request_ref",
        response_snapshot_ref,
        assessment_spec_ref: "assessment_spec_ref",
        instrument_version_ref: "instrument_version_ref",
        scoring_version_ref: "scoring_version_ref",
        calibration_reference: "calibration_reference",
        norm_version_ref: Some("norm_version_ref"),
        requested_output_schema_version: 1,
    }
}

#[test]
fn scoring_dispatch_requires_a_durably_bound_nonempty_snapshot() {
    let unbound = ledger_with_one_response()
        .freeze(SessionState::Completed)
        .unwrap();
    let unbound_error =
        ScoringRequest::from_snapshot(&unbound, scoring_input("response_snapshot_ref"))
            .unwrap_err();
    assert_eq!(unbound_error, ScoringContractError::UnboundResponseSnapshot);
    assert_eq!(
        unbound_error.to_string(),
        "scoring requires a durable response snapshot reference"
    );

    let empty_bound = ResponseLedger::new("session_ref")
        .unwrap()
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap();
    let empty_error =
        ScoringRequest::from_snapshot(&empty_bound, scoring_input("response_snapshot_ref"))
            .unwrap_err();
    assert_eq!(empty_error, ScoringContractError::EmptyResponseSnapshot);
    assert_eq!(
        empty_error.to_string(),
        "scoring requires at least one response event"
    );
}

#[test]
fn scoring_dispatch_rejects_snapshot_reference_substitution() {
    let snapshot = ledger_with_one_response()
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap();

    assert_eq!(snapshot.snapshot_ref(), Some("response_snapshot_ref"));
    let mismatch_error =
        ScoringRequest::from_snapshot(&snapshot, scoring_input("other_snapshot_ref")).unwrap_err();
    assert_eq!(
        mismatch_error,
        ScoringContractError::ResponseSnapshotMismatch
    );
    assert_eq!(
        mismatch_error.to_string(),
        "scoring response snapshot reference does not match supplied snapshot"
    );
}

#[test]
fn scoring_dispatch_rejects_whitespace_aliases_before_identity_comparison() {
    let snapshot = ledger_with_one_response()
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap();

    let mut cases = Vec::new();

    let mut input = scoring_input("response_snapshot_ref");
    input.scoring_request_ref = " scoring_request_ref ";
    cases.push(input);

    let mut input = scoring_input(" response_snapshot_ref ");
    input.assessment_spec_ref = "assessment_spec_ref";
    cases.push(input);

    let mut input = scoring_input("response_snapshot_ref");
    input.assessment_spec_ref = " assessment_spec_ref ";
    cases.push(input);

    let mut input = scoring_input("response_snapshot_ref");
    input.instrument_version_ref = " instrument_version_ref ";
    cases.push(input);

    let mut input = scoring_input("response_snapshot_ref");
    input.scoring_version_ref = " scoring_version_ref ";
    cases.push(input);

    let mut input = scoring_input("response_snapshot_ref");
    input.calibration_reference = " calibration_reference ";
    cases.push(input);

    let mut input = scoring_input("response_snapshot_ref");
    input.norm_version_ref = Some(" norm_version_ref ");
    cases.push(input);

    for input in cases {
        assert_eq!(
            ScoringRequest::from_snapshot(&snapshot, input),
            Err(ScoringContractError::EmptyReference)
        );
    }

    let request =
        ScoringRequest::from_snapshot(&snapshot, scoring_input("response_snapshot_ref")).unwrap();
    assert_eq!(request.scoring_request_ref(), "scoring_request_ref");
    assert_eq!(request.response_snapshot_ref(), "response_snapshot_ref");
    assert_eq!(request.assessment_spec_ref(), "assessment_spec_ref");
    assert_eq!(request.instrument_version_ref(), "instrument_version_ref");
    assert_eq!(request.scoring_version_ref(), "scoring_version_ref");
    assert_eq!(request.calibration_reference(), "calibration_reference");
    assert_eq!(request.norm_version_ref(), Some("norm_version_ref"));
}

#[test]
fn result_identity_and_consent_comparisons_require_canonical_references() {
    let snapshot = ledger_with_one_response()
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap();
    let request =
        ScoringRequest::from_snapshot(&snapshot, scoring_input("response_snapshot_ref")).unwrap();
    let result = ScoringResult::new(
        "scoring_result_ref",
        &request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("construct_ref", 1.0, None).unwrap()],
    )
    .unwrap();

    assert_eq!(result.scoring_result_ref(), "scoring_result_ref");
    assert_eq!(result.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(result.observations()[0].construct_ref(), "construct_ref");

    assert_eq!(
        ScoringResult::new(
            " scoring_result_ref ",
            &request,
            ENGINE_DIGEST,
            vec![ScoreObservation::scored("construct_ref_2", 1.0, None).unwrap()],
        ),
        Err(ScoringContractError::EmptyReference)
    );
    assert_eq!(
        ScoreObservation::scored(" construct_ref ", 1.0, None),
        Err(ScoringContractError::EmptyReference)
    );

    for input in [
        ResultSnapshotInput {
            result_snapshot_ref: " result_snapshot_ref ",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: " participant_ref ",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: " narrative_version_ref ",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &[" consent_ref "],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: Some(" prior_result_ref "),
        },
    ] {
        assert_eq!(
            ResultSnapshot::new(&request, &result, input),
            Err(ResultSnapshotError::EmptyReference)
        );
    }

    let duplicate = ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref", "consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: None,
        },
    )
    .unwrap_err();
    assert_eq!(duplicate, ResultSnapshotError::DuplicateConsentSnapshot);

    let self_supersession = ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: Some("result_snapshot_ref"),
        },
    )
    .unwrap_err();
    assert_eq!(self_supersession, ResultSnapshotError::SelfSupersession);

    let canonical = ResultSnapshot::new(
        &request,
        &result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_ref",
            participant_ref: "participant_ref",
            narrative_version_ref: "narrative_version_ref",
            consent_snapshot_refs: &["consent_ref"],
            created_at_unix_ms: 1,
            supersedes_ref: Some("prior_result_ref"),
        },
    )
    .unwrap();
    assert_eq!(canonical.result_snapshot_ref(), "result_snapshot_ref");
    assert_eq!(canonical.participant_ref(), "participant_ref");
    assert_eq!(canonical.narrative_version_ref(), "narrative_version_ref");
    assert_eq!(canonical.consent_snapshot_refs(), ["consent_ref"]);
    assert_eq!(canonical.supersedes_ref(), Some("prior_result_ref"));
}
