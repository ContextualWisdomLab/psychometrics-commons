use psychometrics_commons_runtime::session::{transition, SessionCommand, SessionState};

#[test]
fn happy_path_reaches_released() {
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
}

#[test]
fn pause_resume_is_reversible_only_before_completion() {
    let active = transition(SessionState::Created, SessionCommand::Activate).unwrap();
    let paused = transition(active, SessionCommand::Pause).unwrap();
    assert_eq!(paused, SessionState::Paused);
    assert!(!paused.accepts_responses());

    let resumed = transition(paused, SessionCommand::Resume).unwrap();
    assert_eq!(resumed, SessionState::Active);
    assert!(resumed.accepts_responses());

    let completed = transition(resumed, SessionCommand::Complete).unwrap();
    assert!(transition(completed, SessionCommand::Resume).is_err());
}

#[test]
fn repeated_commands_are_idempotent() {
    let cases = [
        (SessionState::Active, SessionCommand::Activate),
        (SessionState::Paused, SessionCommand::Pause),
        (SessionState::Completed, SessionCommand::Complete),
        (SessionState::Scoring, SessionCommand::BeginScoring),
        (SessionState::Scored, SessionCommand::RecordScore),
        (SessionState::Released, SessionCommand::Release),
        (SessionState::Expired, SessionCommand::Expire),
        (SessionState::Cancelled, SessionCommand::Cancel),
        (SessionState::Invalidated, SessionCommand::Invalidate),
    ];

    for (state, command) in cases {
        assert_eq!(transition(state, command).unwrap(), state);
    }
}

#[test]
fn terminal_states_reject_unrelated_commands() {
    let cases = [
        (SessionState::Released, SessionCommand::Activate),
        (SessionState::Expired, SessionCommand::Activate),
        (SessionState::Cancelled, SessionCommand::Activate),
        (SessionState::Invalidated, SessionCommand::Activate),
    ];

    for (state, command) in cases {
        assert!(transition(state, command).is_err());
    }
}

#[test]
fn invalid_transition_preserves_source_and_command() {
    let error = transition(SessionState::Created, SessionCommand::Release).unwrap_err();
    assert_eq!(error.state(), SessionState::Created);
    assert_eq!(error.command(), SessionCommand::Release);
    assert_eq!(
        error.to_string(),
        "command Release is not valid while session is Created"
    );
}

#[test]
fn cancellation_expiry_and_invalidation_are_explicit() {
    assert_eq!(
        transition(SessionState::Created, SessionCommand::Cancel).unwrap(),
        SessionState::Cancelled
    );
    assert_eq!(
        transition(SessionState::Active, SessionCommand::Expire).unwrap(),
        SessionState::Expired
    );
    assert_eq!(
        transition(SessionState::Completed, SessionCommand::Invalidate).unwrap(),
        SessionState::Invalidated
    );
}
