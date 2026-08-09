use psychometrics_commons_runtime::session::{transition, SessionCommand, SessionState};

const STATES: [SessionState; 10] = [
    SessionState::Created,
    SessionState::Active,
    SessionState::Paused,
    SessionState::Completed,
    SessionState::Scoring,
    SessionState::Scored,
    SessionState::Released,
    SessionState::Expired,
    SessionState::Cancelled,
    SessionState::Invalidated,
];

const COMMANDS: [SessionCommand; 10] = [
    SessionCommand::Activate,
    SessionCommand::Pause,
    SessionCommand::Resume,
    SessionCommand::Complete,
    SessionCommand::BeginScoring,
    SessionCommand::RecordScore,
    SessionCommand::Release,
    SessionCommand::Expire,
    SessionCommand::Cancel,
    SessionCommand::Invalidate,
];

#[rustfmt::skip]
const VALID: [(SessionState, SessionCommand, SessionState); 28] = [
    (SessionState::Created, SessionCommand::Activate, SessionState::Active),
    (SessionState::Created, SessionCommand::Cancel, SessionState::Cancelled),
    (SessionState::Created, SessionCommand::Expire, SessionState::Expired),
    (SessionState::Created, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Active, SessionCommand::Activate, SessionState::Active),
    (SessionState::Active, SessionCommand::Pause, SessionState::Paused),
    (SessionState::Active, SessionCommand::Complete, SessionState::Completed),
    (SessionState::Active, SessionCommand::Cancel, SessionState::Cancelled),
    (SessionState::Active, SessionCommand::Expire, SessionState::Expired),
    (SessionState::Active, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Paused, SessionCommand::Pause, SessionState::Paused),
    (SessionState::Paused, SessionCommand::Resume, SessionState::Active),
    (SessionState::Paused, SessionCommand::Cancel, SessionState::Cancelled),
    (SessionState::Paused, SessionCommand::Expire, SessionState::Expired),
    (SessionState::Paused, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Completed, SessionCommand::Complete, SessionState::Completed),
    (SessionState::Completed, SessionCommand::BeginScoring, SessionState::Scoring),
    (SessionState::Completed, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Scoring, SessionCommand::BeginScoring, SessionState::Scoring),
    (SessionState::Scoring, SessionCommand::RecordScore, SessionState::Scored),
    (SessionState::Scoring, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Scored, SessionCommand::RecordScore, SessionState::Scored),
    (SessionState::Scored, SessionCommand::Release, SessionState::Released),
    (SessionState::Scored, SessionCommand::Invalidate, SessionState::Invalidated),
    (SessionState::Released, SessionCommand::Release, SessionState::Released),
    (SessionState::Expired, SessionCommand::Expire, SessionState::Expired),
    (SessionState::Cancelled, SessionCommand::Cancel, SessionState::Cancelled),
    (SessionState::Invalidated, SessionCommand::Invalidate, SessionState::Invalidated),
];

#[test]
fn every_documented_transition_reaches_its_expected_state() {
    for (state, command, expected) in VALID {
        assert_eq!(transition(state, command).unwrap(), expected);
    }
}

#[test]
fn every_undocumented_transition_fails_closed() {
    for state in STATES {
        for command in COMMANDS {
            let is_valid = VALID
                .iter()
                .any(|(source, candidate, _)| *source == state && *candidate == command);
            if !is_valid {
                let error = transition(state, command).unwrap_err();
                assert_eq!(error.state(), state);
                assert_eq!(error.command(), command);
            }
        }
    }
}

#[test]
fn canonical_flow_reaches_a_terminal_released_state() {
    let commands = [
        SessionCommand::Activate,
        SessionCommand::Complete,
        SessionCommand::BeginScoring,
        SessionCommand::RecordScore,
        SessionCommand::Release,
    ];
    let state = commands
        .into_iter()
        .try_fold(SessionState::Created, transition)
        .expect("canonical session flow must be valid");

    assert_eq!(state, SessionState::Released);
    assert!(state.is_terminal());
    assert!(!state.accepts_responses());
}

#[test]
fn only_active_sessions_accept_response_events() {
    for state in STATES {
        assert_eq!(state.accepts_responses(), state == SessionState::Active);
    }
}

#[test]
fn only_release_expiry_cancellation_and_invalidation_are_terminal() {
    for state in STATES {
        let expected = matches!(
            state,
            SessionState::Released
                | SessionState::Expired
                | SessionState::Cancelled
                | SessionState::Invalidated
        );
        assert_eq!(state.is_terminal(), expected);
    }
}

#[test]
fn transition_error_has_stable_human_readable_context() {
    let error = transition(SessionState::Created, SessionCommand::Release).unwrap_err();
    assert_eq!(
        error.to_string(),
        "command Release is not valid while session is Created"
    );
}
