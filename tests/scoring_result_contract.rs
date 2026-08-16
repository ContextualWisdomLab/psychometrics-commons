//! Integration tests for scoring-dispatch and immutable result provenance.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringContractError, ScoringRequest,
    ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn completed_snapshot() -> psychometrics_commons_runtime::response::ResponseSnapshot {
    let mut ledger = ResponseLedger::new("session_ref").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_ref",
                client_event_ref: "client_event_ref",
                item_version_ref: "item_version_ref",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "response_snapshot_ref")
        .unwrap()
}

fn scoring_input<'a>() -> ScoringRequestInput<'a> {
    ScoringRequestInput {
        scoring_request_ref: "scoring_request_ref",
        response_snapshot_ref: "response_snapshot_ref",
        assessment_spec_ref: "assessment_spec_ref",
        instrument_version_ref: "instrument_version_ref",
        scoring_version_ref: "scoring_version_ref",
        calibration_reference: "calibration_reference",
        norm_version_ref: Some("norm_version_ref"),
        requested_output_schema_version: 1,
    }
}

fn scoring_request() -> ScoringRequest {
    ScoringRequest::from_snapshot(&completed_snapshot(), scoring_input()).unwrap()
}

fn scored_observation() -> ScoreObservation {
    ScoreObservation::scored("big_five_openness", 0.0, Some(0.25)).unwrap()
}

fn scoring_result() -> ScoringResult {
    ScoringResult::new(
        "scoring_result_ref",
        &scoring_request(),
        ENGINE_DIGEST,
        vec![scored_observation()],
    )
    .unwrap()
}

#[test]
fn scoring_request_pins_completed_snapshot_and_version_bundle() {
    let snapshot = completed_snapshot();
    let request = ScoringRequest::from_snapshot(&snapshot, scoring_input()).unwrap();

    assert_eq!(request.scoring_request_ref(), "scoring_request_ref");
    assert_eq!(request.session_ref(), "session_ref");
    assert_eq!(request.response_snapshot_ref(), "response_snapshot_ref");
    assert_eq!(request.assessment_spec_ref(), "assessment_spec_ref");
    assert_eq!(request.instrument_version_ref(), "instrument_version_ref");
    assert_eq!(request.scoring_version_ref(), "scoring_version_ref");
    assert_eq!(request.calibration_reference(), "calibration_reference");
    assert_eq!(request.norm_version_ref(), Some("norm_version_ref"));
    assert_eq!(request.requested_output_schema_version(), 1);
}

#[test]
fn scoring_request_accepts_absent_norm_but_rejects_invalid_references_or_schema() {
    let snapshot = completed_snapshot();
    let mut without_norm = scoring_input();
    without_norm.norm_version_ref = None;
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, without_norm)
            .unwrap()
            .norm_version_ref(),
        None
    );

    let mut blank_required = scoring_input();
    blank_required.assessment_spec_ref = " assessment_spec_ref ";
    let empty_error = ScoringRequest::from_snapshot(&snapshot, blank_required).unwrap_err();
    assert_eq!(empty_error, ScoringContractError::EmptyReference);
    assert_eq!(
        empty_error.to_string(),
        "scoring contract references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters"
    );

    let mut blank_norm = scoring_input();
    blank_norm.norm_version_ref = Some("");
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, blank_norm).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    for unsupported_version in [0, 2] {
        let mut bad_schema = scoring_input();
        bad_schema.requested_output_schema_version = unsupported_version;
        let schema_error = ScoringRequest::from_snapshot(&snapshot, bad_schema).unwrap_err();
        assert_eq!(
            schema_error,
            ScoringContractError::UnsupportedOutputSchemaVersion
        );
        assert_eq!(
            schema_error.to_string(),
            "requested scoring output schema version is unsupported"
        );
    }
}

#[test]
fn scoring_request_rejects_each_blank_required_reference() {
    let snapshot = completed_snapshot();

    let mut input = scoring_input();
    input.scoring_request_ref = "";
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = scoring_input();
    input.response_snapshot_ref = "";
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = scoring_input();
    input.instrument_version_ref = "";
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = scoring_input();
    input.scoring_version_ref = "";
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, input).unwrap_err(),
        ScoringContractError::EmptyReference
    );

    let mut input = scoring_input();
    input.calibration_reference = "";
    assert_eq!(
        ScoringRequest::from_snapshot(&snapshot, input).unwrap_err(),
        ScoringContractError::EmptyReference
    );
}

#[test]
fn score_observations_preserve_zero_and_distinguish_non_scored_dispositions() {
    let scored = ScoreObservation::scored("construct_ref", 0.0, Some(0.0)).unwrap();
    assert_eq!(scored.construct_ref(), "construct_ref");
    assert_eq!(scored.disposition(), ObservationDisposition::Scored);
    assert_eq!(scored.score(), Some(0.0));
    assert_eq!(scored.standard_error(), Some(0.0));

    for disposition in [
        ObservationDisposition::Abstained,
        ObservationDisposition::Failed,
        ObservationDisposition::Excluded,
    ] {
        let observation = ScoreObservation::without_score("construct_ref", disposition).unwrap();
        assert_eq!(observation.disposition(), disposition);
        assert_eq!(observation.score(), None);
        assert_eq!(observation.standard_error(), None);
    }
}

#[test]
fn score_observations_fail_closed_for_invalid_numeric_or_reference_input() {
    assert_eq!(
        ScoreObservation::scored("", 1.0, None).unwrap_err(),
        ScoringContractError::EmptyReference
    );
    let score_error = ScoreObservation::scored("construct_ref", f64::NAN, None).unwrap_err();
    assert_eq!(score_error, ScoringContractError::InvalidScore);
    assert_eq!(score_error.to_string(), "score values must be finite");

    for invalid_standard_error in [-0.1, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let error = ScoreObservation::scored("construct_ref", 1.0, Some(invalid_standard_error))
            .unwrap_err();
        assert_eq!(error, ScoringContractError::InvalidStandardError);
        assert_eq!(
            error.to_string(),
            "score standard errors must be finite and non-negative"
        );
    }

    assert_eq!(
        ScoreObservation::without_score(" ", ObservationDisposition::Failed).unwrap_err(),
        ScoringContractError::EmptyReference
    );
    assert_eq!(
        ScoreObservation::without_score("construct_ref", ObservationDisposition::Scored)
            .unwrap_err(),
        ScoringContractError::ScoredDispositionRequiresScore
    );
    assert_eq!(
        ScoringContractError::ScoredDispositionRequiresScore.to_string(),
        "scored observations require a numeric score"
    );
}

#[test]
fn scoring_result_pins_engine_request_and_observations() {
    let request = scoring_request();
    let observations = vec![
        scored_observation(),
        ScoreObservation::without_score("big_five_neuroticism", ObservationDisposition::Abstained)
            .unwrap(),
    ];
    let result = ScoringResult::new(
        "scoring_result_ref",
        &request,
        ENGINE_DIGEST,
        observations.clone(),
    )
    .unwrap();

    assert_eq!(result.scoring_result_ref(), "scoring_result_ref");
    assert_eq!(result.scoring_request_ref(), "scoring_request_ref");
    assert_eq!(result.response_snapshot_ref(), "response_snapshot_ref");
    assert_eq!(result.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(result.observations(), observations.as_slice());
}

#[test]
fn scoring_result_rejects_missing_identity_empty_observations_and_duplicate_constructs() {
    let request = scoring_request();
    assert_eq!(
        ScoringResult::new("", &request, ENGINE_DIGEST, vec![scored_observation()]).unwrap_err(),
        ScoringContractError::EmptyReference
    );
    assert_eq!(
        ScoringResult::new(
            "scoring_result_ref",
            &request,
            " ",
            vec![scored_observation()]
        )
        .unwrap_err(),
        ScoringContractError::InvalidEngineArtifactDigest
    );

    let no_observations =
        ScoringResult::new("scoring_result_ref", &request, ENGINE_DIGEST, Vec::new()).unwrap_err();
    assert_eq!(no_observations, ScoringContractError::EmptyObservationSet);
    assert_eq!(
        no_observations.to_string(),
        "scoring results must contain at least one observation"
    );

    let duplicate = ScoringResult::new(
        "scoring_result_ref",
        &request,
        ENGINE_DIGEST,
        vec![scored_observation(), scored_observation()],
    )
    .unwrap_err();
    assert_eq!(duplicate, ScoringContractError::DuplicateConstruct);
    assert_eq!(
        duplicate.to_string(),
        "scoring results must not contain duplicate construct references"
    );
}

fn result_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_snapshot_ref",
        participant_ref: "participant_ref",
        narrative_version_ref: "narrative_version_ref",
        consent_snapshot_refs: &["service_consent_ref", "research_consent_ref"],
        created_at_unix_ms: 1_786_240_000_000,
        supersedes_ref: Some("prior_result_ref"),
    }
}

#[test]
fn result_snapshot_copies_scientific_provenance_without_recomputing_scores() {
    let request = scoring_request();
    let result = scoring_result();
    let snapshot = ResultSnapshot::new(&request, &result, result_input()).unwrap();

    assert_eq!(snapshot.result_snapshot_ref(), "result_snapshot_ref");
    assert_eq!(snapshot.participant_ref(), "participant_ref");
    assert_eq!(snapshot.scoring_result_ref(), "scoring_result_ref");
    assert_eq!(snapshot.session_ref(), "session_ref");
    assert_eq!(snapshot.response_snapshot_ref(), "response_snapshot_ref");
    assert_eq!(snapshot.assessment_spec_ref(), "assessment_spec_ref");
    assert_eq!(snapshot.instrument_version_ref(), "instrument_version_ref");
    assert_eq!(snapshot.scoring_version_ref(), "scoring_version_ref");
    assert_eq!(snapshot.calibration_reference(), "calibration_reference");
    assert_eq!(snapshot.norm_version_ref(), Some("norm_version_ref"));
    assert_eq!(snapshot.requested_output_schema_version(), 1);
    assert_eq!(snapshot.narrative_version_ref(), "narrative_version_ref");
    assert_eq!(
        snapshot.consent_snapshot_refs(),
        ["service_consent_ref", "research_consent_ref"]
    );
    assert_eq!(snapshot.engine_artifact_digest(), ENGINE_DIGEST);
    assert_eq!(snapshot.score_observations(), result.observations());
    assert_eq!(snapshot.created_at_unix_ms(), 1_786_240_000_000);
    assert_eq!(snapshot.supersedes_ref(), Some("prior_result_ref"));
}

#[test]
fn result_snapshot_accepts_first_result_without_supersession() {
    let request = scoring_request();
    let result = scoring_result();
    let mut input = result_input();
    input.supersedes_ref = None;
    let snapshot = ResultSnapshot::new(&request, &result, input).unwrap();
    assert_eq!(snapshot.supersedes_ref(), None);
}

#[test]
fn result_snapshot_rejects_mismatched_scoring_request() {
    let first_request = scoring_request();
    let mut second_input = scoring_input();
    second_input.scoring_request_ref = "other_request_ref";
    let second_request =
        ScoringRequest::from_snapshot(&completed_snapshot(), second_input).unwrap();
    let result = ScoringResult::new(
        "scoring_result_ref",
        &second_request,
        ENGINE_DIGEST,
        vec![scored_observation()],
    )
    .unwrap();

    let error = ResultSnapshot::new(&first_request, &result, result_input()).unwrap_err();
    assert_eq!(error, ResultSnapshotError::ScoringRequestMismatch);
    assert_eq!(
        error.to_string(),
        "scoring result does not belong to the supplied scoring request"
    );
}

#[test]
fn result_snapshot_rejects_invalid_identity_consent_time_or_supersession() {
    let request = scoring_request();
    let result = scoring_result();

    let mut blank_ref = result_input();
    blank_ref.participant_ref = " participant_ref ";
    let empty_error = ResultSnapshot::new(&request, &result, blank_ref).unwrap_err();
    assert_eq!(empty_error, ResultSnapshotError::EmptyReference);
    assert_eq!(
        empty_error.to_string(),
        "result snapshot references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters"
    );

    let mut no_consents = result_input();
    no_consents.consent_snapshot_refs = &[];
    let consent_error = ResultSnapshot::new(&request, &result, no_consents).unwrap_err();
    assert_eq!(consent_error, ResultSnapshotError::MissingConsentSnapshot);
    assert_eq!(
        consent_error.to_string(),
        "result snapshots require at least one consent snapshot reference"
    );

    let mut blank_consent = result_input();
    blank_consent.consent_snapshot_refs = &["service_consent_ref", ""];
    assert_eq!(
        ResultSnapshot::new(&request, &result, blank_consent).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    let mut duplicate_consent = result_input();
    duplicate_consent.consent_snapshot_refs = &["service_consent_ref", "service_consent_ref"];
    let duplicate_error = ResultSnapshot::new(&request, &result, duplicate_consent).unwrap_err();
    assert_eq!(
        duplicate_error,
        ResultSnapshotError::DuplicateConsentSnapshot
    );
    assert_eq!(
        duplicate_error.to_string(),
        "result snapshots must not contain duplicate consent references"
    );

    let mut invalid_time = result_input();
    invalid_time.created_at_unix_ms = 0;
    let time_error = ResultSnapshot::new(&request, &result, invalid_time).unwrap_err();
    assert_eq!(time_error, ResultSnapshotError::InvalidCreationTime);
    assert_eq!(
        time_error.to_string(),
        "result snapshot creation time must be positive"
    );

    let mut blank_supersedes = result_input();
    blank_supersedes.supersedes_ref = Some(" ");
    assert_eq!(
        ResultSnapshot::new(&request, &result, blank_supersedes).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    let mut self_supersedes = result_input();
    self_supersedes.supersedes_ref = Some("result_snapshot_ref");
    let supersession_error = ResultSnapshot::new(&request, &result, self_supersedes).unwrap_err();
    assert_eq!(supersession_error, ResultSnapshotError::SelfSupersession);
    assert_eq!(
        supersession_error.to_string(),
        "a result snapshot cannot supersede itself"
    );
}
