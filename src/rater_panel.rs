//! Governed rater-panel, observation-request, and adjudication aggregates.
//!
//! This bounded context owns hosted product workflow state. It records which
//! exact rater configurations were assigned, which response-evidence references
//! were requested, and whether a separate adjudication case was resolved. It
//! does not create model observations, estimate psychometric parameters,
//! calculate scores, or mutate source rater invocations.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed error returned by governed rater workflow aggregates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RaterWorkflowError {
    /// An opaque identity or evidence reference was not exact and safe.
    InvalidReference,
    /// A panel mutation was attempted after the draft transaction boundary.
    PanelNotDraft,
    /// A panel cannot be published without any assignments.
    EmptyAssignmentSet,
    /// The same assignment identity was added more than once.
    DuplicateAssignmentReference,
    /// The same configuration and repeat index were assigned more than once.
    DuplicateConfigurationRepeat,
    /// The same anchor-response reference was added more than once.
    DuplicateAnchorReference,
    /// A panel lifecycle transition is not valid from its current state.
    InvalidPanelTransition,
    /// An observation request must contain one or more criterion references.
    EmptyCriterionSet,
    /// An observation request repeats a criterion reference.
    DuplicateCriterionReference,
    /// An observation request lifecycle transition is not valid.
    InvalidRequestTransition,
    /// An adjudication case requires at least two source invocations.
    InsufficientAdjudicationSources,
    /// An adjudication case repeats a source invocation reference.
    DuplicateSourceInvocation,
    /// An adjudication case is no longer open for resolution.
    AdjudicationNotOpen,
}

impl Display for RaterWorkflowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "rater workflow references must be exact opaque values",
            Self::PanelNotDraft => "only a draft rater panel may be changed",
            Self::EmptyAssignmentSet => "a rater panel requires an assignment before publication",
            Self::DuplicateAssignmentReference => "rater assignment references must be unique",
            Self::DuplicateConfigurationRepeat => {
                "a rater configuration repeat may be assigned only once"
            }
            Self::DuplicateAnchorReference => "anchor response references must be unique",
            Self::InvalidPanelTransition => "the requested rater panel transition is invalid",
            Self::EmptyCriterionSet => "an observation request requires at least one criterion",
            Self::DuplicateCriterionReference => {
                "observation request criterion references must be unique"
            }
            Self::InvalidRequestTransition => {
                "the requested observation request transition is invalid"
            }
            Self::InsufficientAdjudicationSources => {
                "an adjudication case requires at least two source invocations"
            }
            Self::DuplicateSourceInvocation => {
                "adjudication source invocation references must be unique"
            }
            Self::AdjudicationNotOpen => "only an open adjudication case may be resolved",
        })
    }
}

impl Error for RaterWorkflowError {}

fn exact_reference(reference: &str) -> Result<String, RaterWorkflowError> {
    let Some(normalized) = normalized_reference(reference) else {
        return Err(RaterWorkflowError::InvalidReference);
    };
    if normalized != reference {
        return Err(RaterWorkflowError::InvalidReference);
    }
    Ok(reference.to_owned())
}

/// Lifecycle state of one immutable-revision rater panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RaterPanelState {
    /// Assignments and anchors may still be changed.
    Draft,
    /// Assignments and anchors are frozen for operational use.
    Published,
    /// The panel remains auditable but cannot receive new requests.
    Retired,
}

/// One governed assignment of an exact rater configuration to a panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaterAssignment {
    assignment_ref: String,
    rater_configuration_ref: String,
    repeat_index: u32,
    blind_group_ref: String,
}

impl RaterAssignment {
    /// Create one assignment without treating a repeated invocation as a new rater.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidReference`] when any reference is
    /// blank, normalized differently, numeric-like, or unsafe.
    pub fn new(
        assignment_ref: &str,
        rater_configuration_ref: &str,
        repeat_index: u32,
        blind_group_ref: &str,
    ) -> Result<Self, RaterWorkflowError> {
        Ok(Self {
            assignment_ref: exact_reference(assignment_ref)?,
            rater_configuration_ref: exact_reference(rater_configuration_ref)?,
            repeat_index,
            blind_group_ref: exact_reference(blind_group_ref)?,
        })
    }

    /// Return the panel-local assignment identity.
    #[must_use]
    pub fn assignment_ref(&self) -> &str {
        &self.assignment_ref
    }

    /// Return the exact reusable rater-configuration identity.
    #[must_use]
    pub fn rater_configuration_ref(&self) -> &str {
        &self.rater_configuration_ref
    }

    /// Return the zero-based repeat index within the same configuration.
    #[must_use]
    pub const fn repeat_index(&self) -> u32 {
        self.repeat_index
    }

    /// Return the blind-allocation group identity.
    #[must_use]
    pub fn blind_group_ref(&self) -> &str {
        &self.blind_group_ref
    }
}

/// Aggregate root for one versioned and governable rater panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaterPanelDefinition {
    panel_ref: String,
    panel_revision_ref: String,
    calibration_design_ref: String,
    state: RaterPanelState,
    assignments: Vec<RaterAssignment>,
    anchor_response_refs: Vec<String>,
}

impl RaterPanelDefinition {
    /// Create an empty draft panel bound to one calibration design.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidReference`] when any reference is
    /// not an exact opaque product reference.
    pub fn new(
        panel_ref: &str,
        panel_revision_ref: &str,
        calibration_design_ref: &str,
    ) -> Result<Self, RaterWorkflowError> {
        Ok(Self {
            panel_ref: exact_reference(panel_ref)?,
            panel_revision_ref: exact_reference(panel_revision_ref)?,
            calibration_design_ref: exact_reference(calibration_design_ref)?,
            state: RaterPanelState::Draft,
            assignments: Vec::new(),
            anchor_response_refs: Vec::new(),
        })
    }

    /// Add one configuration assignment while the panel is draft.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::PanelNotDraft`] after publication or
    /// retirement, [`RaterWorkflowError::DuplicateAssignmentReference`] for an
    /// existing assignment identity, or
    /// [`RaterWorkflowError::DuplicateConfigurationRepeat`] when the same exact
    /// configuration and repeat index already exist.
    pub fn add_assignment(
        &mut self,
        assignment: RaterAssignment,
    ) -> Result<(), RaterWorkflowError> {
        if self.state != RaterPanelState::Draft {
            return Err(RaterWorkflowError::PanelNotDraft);
        }
        if self
            .assignments
            .iter()
            .any(|existing| existing.assignment_ref == assignment.assignment_ref)
        {
            return Err(RaterWorkflowError::DuplicateAssignmentReference);
        }
        if self.assignments.iter().any(|existing| {
            existing.rater_configuration_ref == assignment.rater_configuration_ref
                && existing.repeat_index == assignment.repeat_index
        }) {
            return Err(RaterWorkflowError::DuplicateConfigurationRepeat);
        }
        self.assignments.push(assignment);
        Ok(())
    }

    /// Add one anchor-response reference while the panel is draft.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::PanelNotDraft`] after publication or
    /// retirement, [`RaterWorkflowError::InvalidReference`] for an unsafe
    /// reference, or [`RaterWorkflowError::DuplicateAnchorReference`] for a
    /// repeated anchor.
    pub fn add_anchor_response(
        &mut self,
        anchor_response_ref: &str,
    ) -> Result<(), RaterWorkflowError> {
        if self.state != RaterPanelState::Draft {
            return Err(RaterWorkflowError::PanelNotDraft);
        }
        let anchor_response_ref = exact_reference(anchor_response_ref)?;
        if self
            .anchor_response_refs
            .iter()
            .any(|existing| existing == &anchor_response_ref)
        {
            return Err(RaterWorkflowError::DuplicateAnchorReference);
        }
        self.anchor_response_refs.push(anchor_response_ref);
        Ok(())
    }

    /// Freeze assignments and anchors for operational use.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidPanelTransition`] unless the panel
    /// is draft, or [`RaterWorkflowError::EmptyAssignmentSet`] when no rater
    /// configuration has been assigned.
    pub fn publish(&mut self) -> Result<(), RaterWorkflowError> {
        if self.state != RaterPanelState::Draft {
            return Err(RaterWorkflowError::InvalidPanelTransition);
        }
        if self.assignments.is_empty() {
            return Err(RaterWorkflowError::EmptyAssignmentSet);
        }
        self.state = RaterPanelState::Published;
        Ok(())
    }

    /// Retire a published panel without deleting its audit evidence.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidPanelTransition`] unless the panel
    /// is published.
    pub fn retire(&mut self) -> Result<(), RaterWorkflowError> {
        if self.state != RaterPanelState::Published {
            return Err(RaterWorkflowError::InvalidPanelTransition);
        }
        self.state = RaterPanelState::Retired;
        Ok(())
    }

    /// Return the stable panel identity.
    #[must_use]
    pub fn panel_ref(&self) -> &str {
        &self.panel_ref
    }

    /// Return the immutable panel-revision identity.
    #[must_use]
    pub fn panel_revision_ref(&self) -> &str {
        &self.panel_revision_ref
    }

    /// Return the external calibration-design identity.
    #[must_use]
    pub fn calibration_design_ref(&self) -> &str {
        &self.calibration_design_ref
    }

    /// Return the current panel lifecycle state.
    #[must_use]
    pub const fn state(&self) -> RaterPanelState {
        self.state
    }

    /// Return assignments in product-defined insertion order.
    #[must_use]
    pub fn assignments(&self) -> &[RaterAssignment] {
        &self.assignments
    }

    /// Return anchor-response references in product-defined insertion order.
    #[must_use]
    pub fn anchor_response_refs(&self) -> &[String] {
        &self.anchor_response_refs
    }
}

/// Lifecycle state of one observation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservationRequestState {
    /// The request exists but has not crossed the dispatch boundary.
    Pending,
    /// The exact request has been dispatched to an observation producer.
    Dispatched,
    /// One invocation reference was received and frozen.
    Received,
    /// A failure reference was received and retained in the denominator.
    Failed,
}

/// Aggregate root for requesting one rater invocation from one panel assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationRequest {
    request_ref: String,
    panel_revision_ref: String,
    assignment_ref: String,
    response_evidence_ref: String,
    criterion_refs: Vec<String>,
    state: ObservationRequestState,
    invocation_ref: Option<String>,
    failure_ref: Option<String>,
}

impl ObservationRequest {
    /// Create a pending request for a unique non-empty criterion set.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidReference`] for an unsafe
    /// reference, [`RaterWorkflowError::EmptyCriterionSet`] for an empty set,
    /// or [`RaterWorkflowError::DuplicateCriterionReference`] for a repeated
    /// criterion.
    pub fn new(
        request_ref: &str,
        panel_revision_ref: &str,
        assignment_ref: &str,
        response_evidence_ref: &str,
        criterion_refs: &[&str],
    ) -> Result<Self, RaterWorkflowError> {
        if criterion_refs.is_empty() {
            return Err(RaterWorkflowError::EmptyCriterionSet);
        }
        let mut accepted = Vec::with_capacity(criterion_refs.len());
        for criterion_ref in criterion_refs {
            let criterion_ref = exact_reference(criterion_ref)?;
            if accepted.iter().any(|existing| existing == &criterion_ref) {
                return Err(RaterWorkflowError::DuplicateCriterionReference);
            }
            accepted.push(criterion_ref);
        }
        Ok(Self {
            request_ref: exact_reference(request_ref)?,
            panel_revision_ref: exact_reference(panel_revision_ref)?,
            assignment_ref: exact_reference(assignment_ref)?,
            response_evidence_ref: exact_reference(response_evidence_ref)?,
            criterion_refs: accepted,
            state: ObservationRequestState::Pending,
            invocation_ref: None,
            failure_ref: None,
        })
    }

    /// Mark the exact request as dispatched.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidRequestTransition`] unless the
    /// request is pending.
    pub fn dispatch(&mut self) -> Result<(), RaterWorkflowError> {
        if self.state != ObservationRequestState::Pending {
            return Err(RaterWorkflowError::InvalidRequestTransition);
        }
        self.state = ObservationRequestState::Dispatched;
        Ok(())
    }

    /// Freeze the invocation reference returned by the observation context.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidRequestTransition`] unless the
    /// request is dispatched, or [`RaterWorkflowError::InvalidReference`] for
    /// an unsafe invocation reference.
    pub fn receive(&mut self, invocation_ref: &str) -> Result<(), RaterWorkflowError> {
        if self.state != ObservationRequestState::Dispatched {
            return Err(RaterWorkflowError::InvalidRequestTransition);
        }
        self.invocation_ref = Some(exact_reference(invocation_ref)?);
        self.state = ObservationRequestState::Received;
        Ok(())
    }

    /// Freeze one failure reference without discarding the attempted request.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidRequestTransition`] unless the
    /// request is dispatched, or [`RaterWorkflowError::InvalidReference`] for
    /// an unsafe failure reference.
    pub fn fail(&mut self, failure_ref: &str) -> Result<(), RaterWorkflowError> {
        if self.state != ObservationRequestState::Dispatched {
            return Err(RaterWorkflowError::InvalidRequestTransition);
        }
        self.failure_ref = Some(exact_reference(failure_ref)?);
        self.state = ObservationRequestState::Failed;
        Ok(())
    }

    /// Return the request identity.
    #[must_use]
    pub fn request_ref(&self) -> &str {
        &self.request_ref
    }

    /// Return the immutable panel-revision identity.
    #[must_use]
    pub fn panel_revision_ref(&self) -> &str {
        &self.panel_revision_ref
    }

    /// Return the panel-local assignment identity.
    #[must_use]
    pub fn assignment_ref(&self) -> &str {
        &self.assignment_ref
    }

    /// Return the opaque response-evidence identity.
    #[must_use]
    pub fn response_evidence_ref(&self) -> &str {
        &self.response_evidence_ref
    }

    /// Return requested criteria in product-defined order.
    #[must_use]
    pub fn criterion_refs(&self) -> &[String] {
        &self.criterion_refs
    }

    /// Return the current request lifecycle state.
    #[must_use]
    pub const fn state(&self) -> ObservationRequestState {
        self.state
    }

    /// Return the received invocation identity, if successful.
    #[must_use]
    pub fn invocation_ref(&self) -> Option<&str> {
        self.invocation_ref.as_deref()
    }

    /// Return the retained failure identity, if the request failed.
    #[must_use]
    pub fn failure_ref(&self) -> Option<&str> {
        self.failure_ref.as_deref()
    }
}

/// Lifecycle state of a separate adjudication transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AdjudicationState {
    /// Source invocations are frozen and a resolution is pending.
    Open,
    /// An immutable resolution reference has been recorded.
    Resolved,
    /// The case was closed without replacing any source observation.
    Dismissed,
}

/// Aggregate root for human review of multiple immutable rater invocations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjudicationCase {
    case_ref: String,
    panel_revision_ref: String,
    reason_ref: String,
    source_invocation_refs: Vec<String>,
    state: AdjudicationState,
    resolution_ref: Option<String>,
}

impl AdjudicationCase {
    /// Open an adjudication case over at least two unique invocation references.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::InvalidReference`] for an unsafe
    /// reference, [`RaterWorkflowError::InsufficientAdjudicationSources`] when
    /// fewer than two sources are supplied, or
    /// [`RaterWorkflowError::DuplicateSourceInvocation`] for a repeated source.
    pub fn new(
        case_ref: &str,
        panel_revision_ref: &str,
        reason_ref: &str,
        source_invocation_refs: &[&str],
    ) -> Result<Self, RaterWorkflowError> {
        if source_invocation_refs.len() < 2 {
            return Err(RaterWorkflowError::InsufficientAdjudicationSources);
        }
        let mut accepted = Vec::with_capacity(source_invocation_refs.len());
        for invocation_ref in source_invocation_refs {
            let invocation_ref = exact_reference(invocation_ref)?;
            if accepted.iter().any(|existing| existing == &invocation_ref) {
                return Err(RaterWorkflowError::DuplicateSourceInvocation);
            }
            accepted.push(invocation_ref);
        }
        Ok(Self {
            case_ref: exact_reference(case_ref)?,
            panel_revision_ref: exact_reference(panel_revision_ref)?,
            reason_ref: exact_reference(reason_ref)?,
            source_invocation_refs: accepted,
            state: AdjudicationState::Open,
            resolution_ref: None,
        })
    }

    /// Resolve the case by recording an external immutable resolution artifact.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::AdjudicationNotOpen`] unless the case is
    /// open, or [`RaterWorkflowError::InvalidReference`] for an unsafe
    /// resolution reference.
    pub fn resolve(&mut self, resolution_ref: &str) -> Result<(), RaterWorkflowError> {
        if self.state != AdjudicationState::Open {
            return Err(RaterWorkflowError::AdjudicationNotOpen);
        }
        self.resolution_ref = Some(exact_reference(resolution_ref)?);
        self.state = AdjudicationState::Resolved;
        Ok(())
    }

    /// Dismiss the case without changing any source invocation.
    ///
    /// # Errors
    ///
    /// Returns [`RaterWorkflowError::AdjudicationNotOpen`] unless the case is
    /// open.
    pub fn dismiss(&mut self) -> Result<(), RaterWorkflowError> {
        if self.state != AdjudicationState::Open {
            return Err(RaterWorkflowError::AdjudicationNotOpen);
        }
        self.state = AdjudicationState::Dismissed;
        Ok(())
    }

    /// Return the case identity.
    #[must_use]
    pub fn case_ref(&self) -> &str {
        &self.case_ref
    }

    /// Return the immutable panel-revision identity.
    #[must_use]
    pub fn panel_revision_ref(&self) -> &str {
        &self.panel_revision_ref
    }

    /// Return the versioned reason identity.
    #[must_use]
    pub fn reason_ref(&self) -> &str {
        &self.reason_ref
    }

    /// Return frozen source invocation identities.
    #[must_use]
    pub fn source_invocation_refs(&self) -> &[String] {
        &self.source_invocation_refs
    }

    /// Return the adjudication lifecycle state.
    #[must_use]
    pub const fn state(&self) -> AdjudicationState {
        self.state
    }

    /// Return the immutable resolution artifact, if resolved.
    #[must_use]
    pub fn resolution_ref(&self) -> Option<&str> {
        self.resolution_ref.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdjudicationCase, AdjudicationState, ObservationRequest, ObservationRequestState,
        RaterAssignment, RaterPanelDefinition, RaterPanelState, RaterWorkflowError,
    };

    fn assignment(
        assignment_ref: &str,
        configuration_ref: &str,
        repeat_index: u32,
    ) -> RaterAssignment {
        RaterAssignment::new(
            assignment_ref,
            configuration_ref,
            repeat_index,
            "blind_group_alpha",
        )
        .expect("valid assignment")
    }

    #[test]
    fn assignment_preserves_configuration_and_repeat_identity() {
        let assignment = assignment("assignment_alpha", "configuration_alpha", 2);
        assert_eq!(assignment.assignment_ref(), "assignment_alpha");
        assert_eq!(
            assignment.rater_configuration_ref(),
            "configuration_alpha"
        );
        assert_eq!(assignment.repeat_index(), 2);
        assert_eq!(assignment.blind_group_ref(), "blind_group_alpha");
        assert_eq!(
            RaterAssignment::new(" assignment ", "configuration", 0, "blind"),
            Err(RaterWorkflowError::InvalidReference)
        );
    }

    #[test]
    fn panel_freezes_assignments_and_anchors_at_publication() {
        let mut panel = RaterPanelDefinition::new(
            "panel_alpha",
            "panel_revision_alpha",
            "calibration_design_alpha",
        )
        .expect("valid panel");
        assert_eq!(panel.panel_ref(), "panel_alpha");
        assert_eq!(panel.panel_revision_ref(), "panel_revision_alpha");
        assert_eq!(
            panel.calibration_design_ref(),
            "calibration_design_alpha"
        );
        assert_eq!(panel.state(), RaterPanelState::Draft);
        assert_eq!(panel.publish(), Err(RaterWorkflowError::EmptyAssignmentSet));

        panel
            .add_assignment(assignment(
                "assignment_alpha",
                "configuration_alpha",
                0,
            ))
            .expect("first assignment");
        panel
            .add_assignment(assignment(
                "assignment_beta",
                "configuration_alpha",
                1,
            ))
            .expect("repeat assignment");
        panel
            .add_anchor_response("anchor_response_alpha")
            .expect("anchor response");
        assert_eq!(panel.assignments().len(), 2);
        assert_eq!(panel.anchor_response_refs(), ["anchor_response_alpha"]);

        panel.publish().expect("publish panel");
        assert_eq!(panel.state(), RaterPanelState::Published);
        assert_eq!(
            panel.add_assignment(assignment(
                "assignment_gamma",
                "configuration_beta",
                0,
            )),
            Err(RaterWorkflowError::PanelNotDraft)
        );
        assert_eq!(
            panel.add_anchor_response("anchor_response_beta"),
            Err(RaterWorkflowError::PanelNotDraft)
        );
        assert_eq!(
            panel.publish(),
            Err(RaterWorkflowError::InvalidPanelTransition)
        );
        panel.retire().expect("retire panel");
        assert_eq!(panel.state(), RaterPanelState::Retired);
        assert_eq!(
            panel.retire(),
            Err(RaterWorkflowError::InvalidPanelTransition)
        );
    }

    #[test]
    fn panel_rejects_duplicate_assignment_repeat_and_anchor_identities() {
        let mut panel = RaterPanelDefinition::new("panel", "panel_revision", "design")
            .expect("valid panel");
        panel
            .add_assignment(assignment("assignment_a", "configuration_a", 0))
            .expect("first assignment");
        assert_eq!(
            panel.add_assignment(assignment("assignment_a", "configuration_b", 0)),
            Err(RaterWorkflowError::DuplicateAssignmentReference)
        );
        assert_eq!(
            panel.add_assignment(assignment("assignment_b", "configuration_a", 0)),
            Err(RaterWorkflowError::DuplicateConfigurationRepeat)
        );
        panel
            .add_anchor_response("anchor_a")
            .expect("first anchor");
        assert_eq!(
            panel.add_anchor_response("anchor_a"),
            Err(RaterWorkflowError::DuplicateAnchorReference)
        );
        assert_eq!(
            panel.add_anchor_response(" anchor_b "),
            Err(RaterWorkflowError::InvalidReference)
        );
    }

    #[test]
    fn observation_request_preserves_success_or_failure_denominator_state() {
        let mut success = ObservationRequest::new(
            "request_success",
            "panel_revision",
            "assignment",
            "response_evidence",
            &["criterion_a", "criterion_b"],
        )
        .expect("valid request");
        assert_eq!(success.request_ref(), "request_success");
        assert_eq!(success.panel_revision_ref(), "panel_revision");
        assert_eq!(success.assignment_ref(), "assignment");
        assert_eq!(success.response_evidence_ref(), "response_evidence");
        assert_eq!(success.criterion_refs(), ["criterion_a", "criterion_b"]);
        assert_eq!(success.state(), ObservationRequestState::Pending);
        assert_eq!(success.invocation_ref(), None);
        assert_eq!(success.failure_ref(), None);
        assert_eq!(
            success.receive("invocation"),
            Err(RaterWorkflowError::InvalidRequestTransition)
        );
        success.dispatch().expect("dispatch request");
        success.receive("invocation_alpha").expect("receive invocation");
        assert_eq!(success.state(), ObservationRequestState::Received);
        assert_eq!(success.invocation_ref(), Some("invocation_alpha"));
        assert_eq!(
            success.dispatch(),
            Err(RaterWorkflowError::InvalidRequestTransition)
        );

        let mut failure = ObservationRequest::new(
            "request_failure",
            "panel_revision",
            "assignment",
            "response_evidence",
            &["criterion_a"],
        )
        .expect("valid request");
        failure.dispatch().expect("dispatch request");
        failure.fail("provider_timeout_alpha").expect("record failure");
        assert_eq!(failure.state(), ObservationRequestState::Failed);
        assert_eq!(failure.failure_ref(), Some("provider_timeout_alpha"));
        assert_eq!(
            failure.receive("invocation"),
            Err(RaterWorkflowError::InvalidRequestTransition)
        );
    }

    #[test]
    fn observation_request_rejects_invalid_criterion_sets_and_terminal_mutation() {
        assert_eq!(
            ObservationRequest::new(
                "request",
                "panel_revision",
                "assignment",
                "response",
                &[],
            ),
            Err(RaterWorkflowError::EmptyCriterionSet)
        );
        assert_eq!(
            ObservationRequest::new(
                "request",
                "panel_revision",
                "assignment",
                "response",
                &["criterion", "criterion"],
            ),
            Err(RaterWorkflowError::DuplicateCriterionReference)
        );
        assert_eq!(
            ObservationRequest::new(
                " request ",
                "panel_revision",
                "assignment",
                "response",
                &["criterion"],
            ),
            Err(RaterWorkflowError::InvalidReference)
        );

        let mut request = ObservationRequest::new(
            "request",
            "panel_revision",
            "assignment",
            "response",
            &["criterion"],
        )
        .expect("valid request");
        assert_eq!(
            request.fail("failure"),
            Err(RaterWorkflowError::InvalidRequestTransition)
        );
        request.dispatch().expect("dispatch request");
        assert_eq!(
            request.receive(" invocation "),
            Err(RaterWorkflowError::InvalidReference)
        );
        request.fail("failure").expect("record failure");
        assert_eq!(
            request.fail("second_failure"),
            Err(RaterWorkflowError::InvalidRequestTransition)
        );
    }

    #[test]
    fn adjudication_resolution_is_separate_from_source_invocations() {
        let mut adjudication = AdjudicationCase::new(
            "case_alpha",
            "panel_revision_alpha",
            "posterior_decision_risk",
            &["invocation_alpha", "invocation_beta"],
        )
        .expect("valid adjudication case");
        assert_eq!(adjudication.case_ref(), "case_alpha");
        assert_eq!(adjudication.panel_revision_ref(), "panel_revision_alpha");
        assert_eq!(adjudication.reason_ref(), "posterior_decision_risk");
        assert_eq!(
            adjudication.source_invocation_refs(),
            ["invocation_alpha", "invocation_beta"]
        );
        assert_eq!(adjudication.state(), AdjudicationState::Open);
        assert_eq!(adjudication.resolution_ref(), None);

        adjudication
            .resolve("resolution_artifact_alpha")
            .expect("resolve case");
        assert_eq!(adjudication.state(), AdjudicationState::Resolved);
        assert_eq!(
            adjudication.resolution_ref(),
            Some("resolution_artifact_alpha")
        );
        assert_eq!(
            adjudication.resolve("resolution_artifact_beta"),
            Err(RaterWorkflowError::AdjudicationNotOpen)
        );
        assert_eq!(
            adjudication.dismiss(),
            Err(RaterWorkflowError::AdjudicationNotOpen)
        );
    }

    #[test]
    fn adjudication_rejects_insufficient_duplicate_and_unsafe_sources() {
        assert_eq!(
            AdjudicationCase::new("case", "panel", "reason", &["invocation"]),
            Err(RaterWorkflowError::InsufficientAdjudicationSources)
        );
        assert_eq!(
            AdjudicationCase::new(
                "case",
                "panel",
                "reason",
                &["invocation", "invocation"],
            ),
            Err(RaterWorkflowError::DuplicateSourceInvocation)
        );
        assert_eq!(
            AdjudicationCase::new(
                "case",
                "panel",
                "reason",
                &["invocation", " unsafe "],
            ),
            Err(RaterWorkflowError::InvalidReference)
        );

        let mut adjudication = AdjudicationCase::new(
            "case",
            "panel",
            "reason",
            &["invocation_a", "invocation_b"],
        )
        .expect("valid adjudication case");
        assert_eq!(
            adjudication.resolve(" resolution "),
            Err(RaterWorkflowError::InvalidReference)
        );
        adjudication.dismiss().expect("dismiss case");
        assert_eq!(adjudication.state(), AdjudicationState::Dismissed);
        assert_eq!(adjudication.resolution_ref(), None);
    }

    #[test]
    fn error_messages_cover_every_domain_error() {
        let errors = [
            RaterWorkflowError::InvalidReference,
            RaterWorkflowError::PanelNotDraft,
            RaterWorkflowError::EmptyAssignmentSet,
            RaterWorkflowError::DuplicateAssignmentReference,
            RaterWorkflowError::DuplicateConfigurationRepeat,
            RaterWorkflowError::DuplicateAnchorReference,
            RaterWorkflowError::InvalidPanelTransition,
            RaterWorkflowError::EmptyCriterionSet,
            RaterWorkflowError::DuplicateCriterionReference,
            RaterWorkflowError::InvalidRequestTransition,
            RaterWorkflowError::InsufficientAdjudicationSources,
            RaterWorkflowError::DuplicateSourceInvocation,
            RaterWorkflowError::AdjudicationNotOpen,
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}
