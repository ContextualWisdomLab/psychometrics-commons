//! Contract tests for stable scoring-worker terminal event identity.

use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::scoring_worker::{
    bind_scoring_worker_terminal_event, plan_scoring_worker_attempt, require_stable_terminal_event,
    scoring_terminal_event_ref, ScoringEngineAttempt, ScoringTerminalIdentity, ScoringWorkerError,
    ScoringWorkerPlan, ScriptedScoringEngine,
};

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
        "scoring_terminal:result:scoring_job_alpha:result_alpha"
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
        "scoring_terminal:cause:scoring_job_alpha:invalid_scientific_evidence"
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
}

#[test]
fn planner_rewrites_a_minted_envelope_to_the_stable_result_identity() {
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Completed {
        result_ref: "result_alpha",
    });
    let planned = plan_scoring_worker_attempt(
        "scoring_job_alpha",
        engine.evaluate(),
        &event("event_scoring_worker_minted", "scoring_job_alpha"),
    )
    .unwrap();
    let ScoringWorkerPlan::Complete {
        result_ref,
        event: bound,
    } = planned
    else {
        panic!("completed engine must plan a terminal completion");
    };
    assert_eq!(result_ref, "result_alpha");
    assert_eq!(
        bound.event_ref(),
        "scoring_terminal:result:scoring_job_alpha:result_alpha"
    );
    assert_eq!(bound.event_type(), "scoring.result.completed");
    assert_eq!(bound.tenant_ref(), "tenant_scoring_worker");
    require_stable_terminal_event(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("result_alpha"),
        &bound,
    )
    .unwrap();
}

#[test]
fn planner_rewrites_a_minted_envelope_to_the_stable_cause_identity() {
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::PermanentFailure {
        cause_code: "invalid_scientific_evidence",
    });
    let planned = plan_scoring_worker_attempt(
        "scoring_job_alpha",
        engine.evaluate(),
        &event("event_scoring_worker_minted", "scoring_job_alpha"),
    )
    .unwrap();
    let ScoringWorkerPlan::FailPermanently {
        cause_code,
        event: bound,
    } = planned
    else {
        panic!("permanent engine failure must plan a terminal quarantine");
    };
    assert_eq!(cause_code, "invalid_scientific_evidence");
    assert_eq!(
        bound.event_ref(),
        "scoring_terminal:cause:scoring_job_alpha:invalid_scientific_evidence"
    );
}

#[test]
fn planner_does_not_bind_a_terminal_event_for_retryable_engine_failure() {
    let engine = ScriptedScoringEngine::new(ScoringEngineAttempt::Retryable {
        cause_code: "scoring_engine_unavailable",
    });
    let planned = plan_scoring_worker_attempt(
        "scoring_job_alpha",
        engine.evaluate(),
        &event("event_scoring_worker_minted", "scoring_job_alpha"),
    )
    .unwrap();
    assert_eq!(
        planned,
        ScoringWorkerPlan::Retry {
            cause_code: "scoring_engine_unavailable",
        }
    );
    assert_eq!(
        plan_scoring_worker_attempt(
            "scoring_job_alpha",
            ScoringEngineAttempt::Retryable { cause_code: " " },
            &event("event_scoring_worker_minted", "scoring_job_alpha"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
    assert_eq!(
        plan_scoring_worker_attempt(
            " ",
            ScoringEngineAttempt::Retryable {
                cause_code: "scoring_engine_unavailable",
            },
            &event("event_scoring_worker_minted", "scoring_job_alpha"),
        )
        .unwrap_err(),
        ScoringWorkerError::InvalidReference
    );
}

#[test]
fn bind_helper_replaces_only_the_event_identity() {
    let minted = event("event_scoring_worker_minted", "scoring_job_alpha");
    let bound = bind_scoring_worker_terminal_event(
        "scoring_job_alpha",
        ScoringTerminalIdentity::Result("result_alpha"),
        &minted,
    )
    .unwrap();
    assert_eq!(
        bound.event_ref(),
        "scoring_terminal:result:scoring_job_alpha:result_alpha"
    );
    assert_eq!(bound.source(), minted.source());
    assert_eq!(bound.payload_digest(), minted.payload_digest());
    assert_eq!(bound.correlation_ref(), minted.correlation_ref());
}
