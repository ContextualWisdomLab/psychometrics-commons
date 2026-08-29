//! Coverage regressions for governed rater-panel aggregate invariants.

use psychometrics_commons_runtime::rater_panel::{
    AdjudicationCase, AdjudicationState, ObservationRequest, ObservationRequestState,
    RaterAssignment, RaterPanelDefinition, RaterPanelState, RaterWorkflowError,
};

fn assignment(assignment_ref: &str, configuration_ref: &str, repeat_index: u32) -> RaterAssignment {
    RaterAssignment::new(
        assignment_ref,
        configuration_ref,
        repeat_index,
        "blind_group_coverage",
    )
    .expect("valid assignment")
}

#[test]
fn every_assignment_reference_position_rejects_inexact_input() {
    assert_eq!(
        RaterAssignment::new(" assignment ", "configuration", 0, "blind_group"),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        RaterAssignment::new("assignment", " configuration ", 0, "blind_group"),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        RaterAssignment::new("assignment", "configuration", 0, " blind_group "),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        RaterAssignment::new("123", "configuration", 0, "blind_group"),
        Err(RaterWorkflowError::InvalidReference)
    );
}

#[test]
fn every_panel_constructor_reference_position_rejects_inexact_input() {
    assert_eq!(
        RaterPanelDefinition::new(" panel ", "revision", "design"),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        RaterPanelDefinition::new("panel", " revision ", "design"),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        RaterPanelDefinition::new("panel", "revision", " design "),
        Err(RaterWorkflowError::InvalidReference)
    );
}

#[test]
fn retired_panel_remains_frozen_for_every_mutation() {
    let mut panel = RaterPanelDefinition::new("panel", "revision", "design").expect("panel");
    panel
        .add_assignment(assignment("assignment", "configuration", 0))
        .expect("assignment");
    panel.publish().expect("publish");
    panel.retire().expect("retire");

    assert_eq!(panel.state(), RaterPanelState::Retired);
    assert_eq!(
        panel.add_assignment(assignment("other_assignment", "other_configuration", 0)),
        Err(RaterWorkflowError::PanelNotDraft)
    );
    assert_eq!(
        panel.add_anchor_response("other_anchor"),
        Err(RaterWorkflowError::PanelNotDraft)
    );
    assert_eq!(
        panel.publish(),
        Err(RaterWorkflowError::InvalidPanelTransition)
    );
}

#[test]
fn observation_constructor_validates_every_reference_position() {
    let cases = [
        (" request ", "revision", "assignment", "response"),
        ("request", " revision ", "assignment", "response"),
        ("request", "revision", " assignment ", "response"),
        ("request", "revision", "assignment", " response "),
    ];
    for (request_ref, revision_ref, assignment_ref, response_ref) in cases {
        assert_eq!(
            ObservationRequest::new(
                request_ref,
                revision_ref,
                assignment_ref,
                response_ref,
                &["criterion"],
            ),
            Err(RaterWorkflowError::InvalidReference)
        );
    }
    assert_eq!(
        ObservationRequest::new(
            "request",
            "revision",
            "assignment",
            "response",
            &[" criterion "],
        ),
        Err(RaterWorkflowError::InvalidReference)
    );
}

#[test]
fn observation_terminal_states_reject_all_cross_transitions() {
    let mut received = ObservationRequest::new(
        "request_received",
        "revision",
        "assignment",
        "response",
        &["criterion"],
    )
    .expect("request");
    received.dispatch().expect("dispatch");
    received.receive("invocation").expect("receive");
    assert_eq!(received.state(), ObservationRequestState::Received);
    assert_eq!(
        received.fail("failure"),
        Err(RaterWorkflowError::InvalidRequestTransition)
    );
    assert_eq!(received.failure_ref(), None);

    let mut failed = ObservationRequest::new(
        "request_failed",
        "revision",
        "assignment",
        "response",
        &["criterion"],
    )
    .expect("request");
    failed.dispatch().expect("dispatch");
    failed.fail("failure").expect("fail");
    assert_eq!(failed.state(), ObservationRequestState::Failed);
    assert_eq!(
        failed.dispatch(),
        Err(RaterWorkflowError::InvalidRequestTransition)
    );
    assert_eq!(failed.invocation_ref(), None);
}

#[test]
fn dispatched_request_stays_dispatched_after_invalid_terminal_reference() {
    let mut request = ObservationRequest::new(
        "request",
        "revision",
        "assignment",
        "response",
        &["criterion"],
    )
    .expect("request");
    request.dispatch().expect("dispatch");
    assert_eq!(
        request.fail(" failure "),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(request.state(), ObservationRequestState::Dispatched);
    assert_eq!(request.failure_ref(), None);
    request
        .receive("invocation")
        .expect("receive after invalid failure");
}

#[test]
fn adjudication_constructor_validates_every_reference_position() {
    let sources = ["invocation_a", "invocation_b"];
    assert_eq!(
        AdjudicationCase::new(" case ", "revision", "reason", &sources),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        AdjudicationCase::new("case", " revision ", "reason", &sources),
        Err(RaterWorkflowError::InvalidReference)
    );
    assert_eq!(
        AdjudicationCase::new("case", "revision", " reason ", &sources),
        Err(RaterWorkflowError::InvalidReference)
    );
}

#[test]
fn adjudication_terminal_states_reject_opposite_transitions() {
    let mut dismissed = AdjudicationCase::new(
        "dismissed_case",
        "revision",
        "reason",
        &["invocation_a", "invocation_b"],
    )
    .expect("case");
    dismissed.dismiss().expect("dismiss");
    assert_eq!(dismissed.state(), AdjudicationState::Dismissed);
    assert_eq!(
        dismissed.resolve("resolution"),
        Err(RaterWorkflowError::AdjudicationNotOpen)
    );

    let mut resolved = AdjudicationCase::new(
        "resolved_case",
        "revision",
        "reason",
        &["invocation_a", "invocation_b"],
    )
    .expect("case");
    resolved.resolve("resolution").expect("resolve");
    assert_eq!(resolved.state(), AdjudicationState::Resolved);
    assert_eq!(
        resolved.dismiss(),
        Err(RaterWorkflowError::AdjudicationNotOpen)
    );
}
