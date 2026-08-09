//! Integration tests for immutable instrument-publication lifecycle semantics.

use psychometrics_commons_runtime::instrument::{
    transition, PublicationCommand, PublicationError, PublicationState,
};

const STATES: [PublicationState; 5] = [
    PublicationState::Draft,
    PublicationState::Review,
    PublicationState::Published,
    PublicationState::Suspended,
    PublicationState::Retired,
];

const COMMANDS: [PublicationCommand; 5] = [
    PublicationCommand::SubmitReview,
    PublicationCommand::Publish,
    PublicationCommand::Suspend,
    PublicationCommand::Reactivate,
    PublicationCommand::Retire,
];

#[rustfmt::skip]
const VALID: [(PublicationState, PublicationCommand, PublicationState); 10] = [
    (PublicationState::Draft, PublicationCommand::SubmitReview, PublicationState::Review),
    (PublicationState::Review, PublicationCommand::SubmitReview, PublicationState::Review),
    (PublicationState::Review, PublicationCommand::Publish, PublicationState::Published),
    (PublicationState::Published, PublicationCommand::Publish, PublicationState::Published),
    (PublicationState::Published, PublicationCommand::Suspend, PublicationState::Suspended),
    (PublicationState::Published, PublicationCommand::Reactivate, PublicationState::Published),
    (PublicationState::Published, PublicationCommand::Retire, PublicationState::Retired),
    (PublicationState::Suspended, PublicationCommand::Suspend, PublicationState::Suspended),
    (PublicationState::Suspended, PublicationCommand::Reactivate, PublicationState::Published),
    (PublicationState::Suspended, PublicationCommand::Retire, PublicationState::Retired),
];

#[test]
fn documented_publication_transitions_reach_expected_states() {
    for (state, command, expected) in VALID {
        assert_eq!(transition(state, command).unwrap(), expected);
    }
}

#[test]
fn retired_release_accepts_only_idempotent_retirement() {
    assert_eq!(
        transition(PublicationState::Retired, PublicationCommand::Retire).unwrap(),
        PublicationState::Retired
    );

    for command in [
        PublicationCommand::SubmitReview,
        PublicationCommand::Publish,
        PublicationCommand::Suspend,
        PublicationCommand::Reactivate,
    ] {
        let error = transition(PublicationState::Retired, command).unwrap_err();
        assert_eq!(
            error,
            PublicationError::InvalidTransition {
                state: PublicationState::Retired,
                command,
            }
        );
    }
}

#[test]
fn every_other_undocumented_transition_fails_closed() {
    for state in STATES {
        for command in COMMANDS {
            let is_valid = VALID
                .iter()
                .any(|(source, candidate, _)| *source == state && *candidate == command)
                || (state == PublicationState::Retired && command == PublicationCommand::Retire);
            if !is_valid {
                let error = transition(state, command).unwrap_err();
                assert_eq!(
                    error,
                    PublicationError::InvalidTransition { state, command }
                );
            }
        }
    }
}

#[test]
fn only_published_release_accepts_new_sessions() {
    for state in STATES {
        assert_eq!(
            state.accepts_new_sessions(),
            state == PublicationState::Published
        );
    }
}

#[test]
fn only_retired_release_is_terminal() {
    for state in STATES {
        assert_eq!(state.is_terminal(), state == PublicationState::Retired);
    }
}

#[test]
fn suspended_release_can_only_resume_same_immutable_version_or_retire() {
    assert_eq!(
        transition(PublicationState::Suspended, PublicationCommand::Reactivate).unwrap(),
        PublicationState::Published
    );
    assert_eq!(
        transition(PublicationState::Suspended, PublicationCommand::Retire).unwrap(),
        PublicationState::Retired
    );
    assert_eq!(
        transition(PublicationState::Suspended, PublicationCommand::Publish).unwrap_err(),
        PublicationError::InvalidTransition {
            state: PublicationState::Suspended,
            command: PublicationCommand::Publish,
        }
    );
}

#[test]
fn invalid_transition_error_preserves_stable_context() {
    let error = transition(PublicationState::Draft, PublicationCommand::Publish).unwrap_err();
    assert_eq!(error.state(), PublicationState::Draft);
    assert_eq!(error.command(), PublicationCommand::Publish);
    assert_eq!(
        error.to_string(),
        "command Publish is not valid while instrument release is Draft"
    );
}
