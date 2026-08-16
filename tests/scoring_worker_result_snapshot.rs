//! Request-bound scoring-worker planning must persist one immutable result snapshot.

use psychometrics_commons_runtime::result::ResultSnapshotInput;
use psychometrics_commons_runtime::scoring::{
    ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::scoring_worker::{
    plan_scoring_worker_result_attempt, ScoringWorkerEnvelope, ScoringWorkerError,
    ScoringWorkerResultAttempt, ScoringWorkerResultEngine, ScoringWorkerResultOutcome,
    ScoringWorkerResultPlan,
};
use std::cell::Cell;

const ENGINE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn loaded_request() -> ScoringRequest {
    ScoringRequest::from_persisted(
        "session_reload_score",
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_reload_score",
            response_snapshot_ref: "response_snapshot_reload_score",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap()
}

fn scored_result(request: &ScoringRequest) -> ScoringResult {
    ScoringResult::new(
        "result_reload_score",
        request,
        ENGINE_DIGEST,
        vec![ScoreObservation::scored("big_five_openness", 1.2, Some(0.15)).unwrap()],
    )
    .unwrap()
}

fn snapshot_input<'a>() -> ResultSnapshotInput<'a> {
    ResultSnapshotInput {
        result_snapshot_ref: "result_reload_score",
        participant_ref: "participant_reload_score",
        narrative_version_ref: "narrative_version_big_five_v1",
        consent_snapshot_refs: &["consent_snapshot_service_v1"],
        created_at_unix_ms: 70_000,
        supersedes_ref: None,
    }
}

fn worker_envelope() -> ScoringWorkerEnvelope<'static> {
    ScoringWorkerEnvelope {
        event_type: "scoring.result.completed",
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_scoring_worker",
        occurred_at_unix_ms: 70_000,
        correlation_ref: "correlation_reload_score",
        causation_ref: Some("scoring_request_reload_score"),
        payload_digest: PAYLOAD_DIGEST,
    }
}

struct ScriptedResultEngine {
    expected_job: &'static str,
    expected_request: String,
    result: Result<ScoringWorkerResultOutcome, ScoringWorkerError>,
    calls: Cell<usize>,
}

impl ScoringWorkerResultEngine for ScriptedResultEngine {
    fn score_claimed_request(
        &self,
        scoring_job_ref: &str,
        request: &ScoringRequest,
    ) -> Result<ScoringWorkerResultOutcome, ScoringWorkerError> {
        assert_eq!(scoring_job_ref, self.expected_job);
        assert_eq!(request.scoring_request_ref(), self.expected_request);
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

#[test]
fn planner_builds_a_result_snapshot_bound_to_the_loaded_request() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(scored_result(&request)),
        }),
        calls: Cell::new(0),
    };

    let attempt = unwrap_terminal(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            snapshot_input(),
            worker_envelope(),
        )
        .unwrap(),
    );

    assert_eq!(engine.calls.get(), 1);
    let snapshot = attempt
        .snapshot()
        .expect("completed attempt must carry a snapshot");
    assert_eq!(snapshot.result_snapshot_ref(), "result_reload_score");
    assert_eq!(snapshot.scoring_result_ref(), "result_reload_score");
    assert_eq!(snapshot.session_ref(), "session_reload_score");
    assert_eq!(
        snapshot.response_snapshot_ref(),
        "response_snapshot_reload_score"
    );
    assert_eq!(snapshot.score_observations().len(), 1);
    assert_eq!(snapshot.score_observations()[0].score(), Some(1.2));
    assert_eq!(
        attempt.event().event_ref(),
        "scoring_terminal:result:24:scoring_job_reload_score:19:result_reload_score"
    );
}

#[test]
fn planner_rejects_an_invalid_envelope_before_calling_the_result_engine() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(scored_result(&request)),
        }),
        calls: Cell::new(0),
    };
    let mut envelope = worker_envelope();
    envelope.payload_digest = "not-a-digest";

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            snapshot_input(),
            envelope,
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidEnvelope
    );
    assert_eq!(engine.calls.get(), 0);
}

#[test]
fn planner_rejects_a_result_bound_to_another_request() {
    let request = loaded_request();
    let other = ScoringRequest::from_persisted(
        "session_other_score",
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_other_score",
            response_snapshot_ref: "response_snapshot_other_score",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(scored_result(&other)),
        }),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            snapshot_input(),
            worker_envelope(),
        )
        .unwrap_err(),
        ScoringWorkerError::MismatchedScoringResult
    );
    assert_eq!(engine.calls.get(), 1);
}

#[test]
fn planner_rejects_a_snapshot_identity_that_does_not_reuse_the_engine_result() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(scored_result(&request)),
        }),
        calls: Cell::new(0),
    };
    let mut input = snapshot_input();
    input.result_snapshot_ref = "result_other_score";

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            input,
            worker_envelope(),
        )
        .unwrap_err(),
        ScoringWorkerError::MismatchedScoringResult
    );
}

#[test]
fn planner_rejects_a_snapshot_missing_consent_without_binding_an_event() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Completed {
            result: Box::new(scored_result(&request)),
        }),
        calls: Cell::new(0),
    };
    let mut input = snapshot_input();
    input.consent_snapshot_refs = &[];

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            input,
            worker_envelope(),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidResultSnapshot
    );
}

fn unwrap_terminal(plan: ScoringWorkerResultPlan) -> ScoringWorkerResultAttempt {
    match plan {
        ScoringWorkerResultPlan::Terminal(attempt) => *attempt,
        ScoringWorkerResultPlan::Retryable { cause_code } => {
            panic!("expected a terminal plan, got retryable {cause_code}")
        }
    }
}

#[test]
fn planner_schedules_a_retryable_outage_without_binding_an_event() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Retryable {
            cause_code: "engine_unavailable".to_owned(),
        }),
        calls: Cell::new(0),
    };

    let plan = plan_scoring_worker_result_attempt(
        "scoring_job_reload_score",
        &request,
        &engine,
        snapshot_input(),
        worker_envelope(),
    )
    .unwrap();

    assert_eq!(engine.calls.get(), 1);
    assert_eq!(
        plan,
        ScoringWorkerResultPlan::Retryable {
            cause_code: "engine_unavailable".to_owned(),
        }
    );
}

#[test]
fn planner_rejects_a_blank_retryable_cause() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Retryable {
            cause_code: " ".to_owned(),
        }),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            snapshot_input(),
            worker_envelope(),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
}

#[test]
fn planner_rejects_a_numeric_retryable_cause() {
    let request = loaded_request();
    let engine = ScriptedResultEngine {
        expected_job: "scoring_job_reload_score",
        expected_request: request.scoring_request_ref().to_owned(),
        result: Ok(ScoringWorkerResultOutcome::Retryable {
            cause_code: "123".to_owned(),
        }),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_result_attempt(
            "scoring_job_reload_score",
            &request,
            &engine,
            snapshot_input(),
            worker_envelope(),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
}
