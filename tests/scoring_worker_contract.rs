//! Contract tests for stable scoring-worker terminal event identity.

use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::scoring_worker::{
    plan_scoring_worker_attempt, require_stable_terminal_event, scoring_terminal_event_ref,
    ScoringTerminalIdentity, ScoringWorkerEngine, ScoringWorkerEngineOutcome,
    ScoringWorkerEnvelope, ScoringWorkerError,
};
use std::cell::Cell;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(event_ref: &str, subject_ref: &str) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        "scoring.result.completed",
        "v1",
        "psychometrics_commons",
        "tenant_scoring_worker",
        subject_ref,
        20_000,
        "correlation_scoring_worker",
        Some("scoring_request_worker"),
        DIGEST,
    )
    .unwrap()
}

#[test]
fn terminal_event_ref_is_stable_for_the_same_job_and_result() {
    let first = scoring_terminal_event_ref(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("result_alpha"),
    )
    .unwrap();
    let second = scoring_terminal_event_ref(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("result_alpha"),
    )
    .unwrap();
    assert_eq!(
        first,
        "scoring_terminal:result:17:scoring_job_alpha:12:result_alpha"
    );
    assert_eq!(first, second);
    assert_ne!(
        first,
        scoring_terminal_event_ref(
            "scoring_job_alpha",
            ScoringTerminalIdentity::Result("result_beta"),
        )
        .unwrap()
    );
}

#[test]
fn terminal_event_ref_does_not_collide_when_references_contain_colons() {
    let left = scoring_terminal_event_ref(
        "scoring_job:alpha",
        ScoringTerminalIdentity::Result("result_beta"),
    )
    .unwrap();
    let right = scoring_terminal_event_ref(
        "scoring_job",
        ScoringTerminalIdentity::Result("alpha:result_beta"),
    )
    .unwrap();
    assert_ne!(left, right);
}

#[test]
fn result_and_cause_identities_do_not_share_an_event_ref() {
    let completed = scoring_terminal_event_ref(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("invalid_scientific_evidence"),
    )
    .unwrap();
    let failed = scoring_terminal_event_ref(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Cause("invalid_scientific_evidence"),
    )
    .unwrap();
    assert_eq!(
        failed,
        "scoring_terminal:cause:17:scoring_job_alpha:27:invalid_scientific_evidence"
    );
    assert_ne!(completed, failed);
}

#[test]
fn terminal_event_ref_rejects_numeric_like_or_blank_identity() {
    assert_eq!(
        scoring_terminal_event_ref("123", ScoringTerminalIdentity::Result("result_alpha"))
            .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(
        scoring_terminal_event_ref("scoring_job_alpha", ScoringTerminalIdentity::Result("1e5"))
            .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(
        scoring_terminal_event_ref("scoring_job_alpha", ScoringTerminalIdentity::Cause(" "))
            .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
}

#[test]
fn worker_accepts_only_the_stable_event_identity() {
    let identity = ScoringTerminalIdentity::Result("result_alpha");
    let event_ref = scoring_terminal_event_ref("scoring_job_alpha", identity).unwrap();
    require_stable_terminal_event(
        "scoring_job_alpha",
        identity,
        &event(&event_ref, "scoring_job_alpha"),
    )
    .unwrap();
    assert_eq!(
        require_stable_terminal_event(
            "scoring_job_alpha",
            identity,
            &event("event_scoring_worker_minted", "scoring_job_alpha"),
        )
        .unwrap_err(),
        ScoringWorkerError::UnstableEventRef
    );
}

#[test]
fn scoring_worker_errors_explain_the_next_safe_action() {
    assert_eq!(
        ScoringWorkerError::InvalidReference.to_string(),
        "scoring worker identities must be opaque non-numeric values"
    );
    assert_eq!(
        ScoringWorkerError::UnstableEventRef.to_string(),
        "scoring worker must reuse the stable job and outcome event identity"
    );
    assert_eq!(
        ScoringWorkerError::InvalidEnvelope.to_string(),
        "scoring worker envelope fields must be valid integration evidence"
    );
}

struct ScriptedScoringEngine {
    expected_job: &'static str,
    expected_request: &'static str,
    result: Result<ScoringWorkerEngineOutcome, ScoringWorkerError>,
    calls: Cell<usize>,
}

impl ScoringWorkerEngine for ScriptedScoringEngine {
    fn score_claimed_job(
        &self,
        scoring_job_ref: &str,
        scoring_request_ref: &str,
    ) -> Result<ScoringWorkerEngineOutcome, ScoringWorkerError> {
        assert_eq!(scoring_job_ref, self.expected_job);
        assert_eq!(scoring_request_ref, self.expected_request);
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

fn worker_envelope(event_type: &str) -> ScoringWorkerEnvelope<'_> {
    ScoringWorkerEnvelope {
        event_type,
        schema_version: "v1",
        source: "psychometrics_commons",
        tenant_ref: "tenant_scoring_worker",
        occurred_at_unix_ms: 20_000,
        correlation_ref: "correlation_scoring_worker",
        causation_ref: Some("scoring_request_worker"),
        payload_digest: DIGEST,
    }
}

#[test]
fn planner_binds_the_stable_result_event_and_ignores_a_minted_identity() {
    let engine = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_alpha",
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_alpha".to_owned(),
        }),
        calls: Cell::new(0),
    };

    let attempt = plan_scoring_worker_attempt(
        "scoring_job_alpha",
        "scoring_request_alpha",
        &engine,
        worker_envelope("scoring.result.completed"),
    )
    .unwrap();

    assert_eq!(engine.calls.get(), 1);
    assert_eq!(
        attempt.event().event_ref(),
        "scoring_terminal:result:17:scoring_job_alpha:12:result_alpha"
    );
    assert_eq!(attempt.event().subject_ref(), "scoring_job_alpha");
    assert_eq!(attempt.event().event_type(), "scoring.result.completed");
    assert_eq!(
        attempt.outcome(),
        &ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_alpha".to_owned(),
        }
    );
    require_stable_terminal_event(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("result_alpha"),
        attempt.event(),
    )
    .unwrap();
}

#[test]
fn planner_binds_a_permanent_scientific_failure_to_the_stable_cause_identity() {
    let engine = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_unknown",
        result: Ok(ScoringWorkerEngineOutcome::Failed {
            cause_code: "invalid_scientific_evidence".to_owned(),
        }),
        calls: Cell::new(0),
    };

    let attempt = plan_scoring_worker_attempt(
        "scoring_job_alpha",
        "scoring_request_unknown",
        &engine,
        worker_envelope("scoring.result.failed"),
    )
    .unwrap();

    assert_eq!(engine.calls.get(), 1);
    assert_eq!(
        attempt.event().event_ref(),
        "scoring_terminal:cause:17:scoring_job_alpha:27:invalid_scientific_evidence"
    );
    assert_eq!(attempt.event().event_type(), "scoring.result.failed");
    require_stable_terminal_event(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Cause("invalid_scientific_evidence"),
        attempt.event(),
    )
    .unwrap();
}

#[test]
fn planner_rejects_blank_request_identity_before_calling_the_engine() {
    let engine = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "unused",
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_alpha".to_owned(),
        }),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            " ",
            &engine,
            worker_envelope("scoring.result.completed"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(engine.calls.get(), 0);
}

#[test]
fn planner_rejects_an_invalid_caller_envelope_after_the_engine_returns() {
    let engine = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_alpha",
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_alpha".to_owned(),
        }),
        calls: Cell::new(0),
    };
    let mut envelope = worker_envelope("scoring.result.completed");
    envelope.payload_digest = "not-a-digest";

    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            "scoring_request_alpha",
            &engine,
            envelope,
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidEnvelope
    );
    assert_eq!(engine.calls.get(), 1);
}

#[test]
fn planner_rejects_invalid_job_identity_before_calling_the_engine() {
    let engine = ScriptedScoringEngine {
        expected_job: "unused",
        expected_request: "unused",
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: "result_alpha".to_owned(),
        }),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_attempt(
            "123",
            "scoring_request_alpha",
            &engine,
            worker_envelope("scoring.result.completed"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(engine.calls.get(), 0);
}

#[test]
fn planner_returns_a_non_terminal_engine_error_without_binding_an_event() {
    let engine = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_unknown",
        result: Err(ScoringWorkerError::InvalidEnvelope),
        calls: Cell::new(0),
    };

    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            "scoring_request_unknown",
            &engine,
            worker_envelope("scoring.result.completed"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidEnvelope
    );
    assert_eq!(engine.calls.get(), 1);
}

#[test]
fn planner_rejects_a_blank_or_numeric_engine_result_identity() {
    let blank = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_alpha",
        result: Ok(ScoringWorkerEngineOutcome::Completed {
            result_ref: " ".to_owned(),
        }),
        calls: Cell::new(0),
    };
    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            "scoring_request_alpha",
            &blank,
            worker_envelope("scoring.result.completed"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(blank.calls.get(), 1);

    let numeric = ScriptedScoringEngine {
        expected_job: "scoring_job_alpha",
        expected_request: "scoring_request_alpha",
        result: Ok(ScoringWorkerEngineOutcome::Failed {
            cause_code: "1e5".to_owned(),
        }),
        calls: Cell::new(0),
    };
    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            "scoring_request_alpha",
            &numeric,
            worker_envelope("scoring.result.failed"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(numeric.calls.get(), 1);
}
