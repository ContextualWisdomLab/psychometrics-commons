//! Wire and durable lifecycle vocabularies must not drift apart.
//!
//! Public HTTP responses and `PostgreSQL` reconstruction describe the same session
//! lifecycle. A future state rename must therefore change both representations in
//! one reviewed workstream rather than silently creating two vocabularies.

use psychometrics_commons_runtime::session::{SessionCommand, SessionState};

#[test]
fn session_state_wire_names_match_persisted_names() {
    let states = [
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

    for state in states {
        assert_eq!(state.as_str(), state.persist_name());
    }
}

#[test]
fn session_command_wire_names_match_persisted_names() {
    let commands = [
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

    for command in commands {
        assert_eq!(command.as_str(), command.persist_name());
    }
}
