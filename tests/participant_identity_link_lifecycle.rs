//! Regression coverage for append-only participant identity-link lifecycle history.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

fn linked_participant() -> ParticipantRecord {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_lifecycle", "tenant_lifecycle", 10_000).unwrap();
    participant
        .link_account(
            "link_event_first",
            "keyverse_issuer_first",
            "keyverse_subject_first",
            "anonymous_proof_first",
            "authenticated_proof_first",
            10_100,
        )
        .unwrap();
    participant
}

#[test]
fn ending_a_link_preserves_identity_and_history_while_clearing_current_projection() {
    let mut participant = linked_participant();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();

    assert_eq!(participant.participant_ref(), "participant_lifecycle");
    assert_eq!(participant.tenant_ref(), "tenant_lifecycle");
    assert_eq!(participant.linked_issuer_ref(), None);
    assert_eq!(participant.linked_subject_ref(), None);
    assert_eq!(participant.link_event_ref(), None);
    assert_eq!(participant.anonymous_proof_ref(), None);
    assert_eq!(participant.authenticated_proof_ref(), None);
    assert_eq!(participant.linked_at_unix_ms(), None);
    assert_eq!(participant.link_history().len(), 1);
    assert_eq!(participant.link_end_history().len(), 1);

    let event = &participant.link_end_history()[0];
    assert_eq!(event.link_end_event_ref(), "link_end_event_first");
    assert_eq!(event.linked_event_ref(), "link_event_first");
    assert_eq!(event.evidence_ref(), "link_end_evidence_first");
    assert_eq!(event.ended_at_unix_ms(), 10_200);
}

#[test]
fn exact_link_end_replay_is_idempotent_and_conflicting_replay_fails_closed() {
    let mut participant = linked_participant();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();

    assert_eq!(participant.link_end_history().len(), 1);
    assert_eq!(
        participant.record_link_end("link_end_event_first", "link_end_evidence_other", 10_200),
        Err(AccountLinkError::ConflictingLinkEndReplay)
    );
    assert_eq!(
        participant.record_link_end("link_end_event_first", "link_end_evidence_first", 10_201),
        Err(AccountLinkError::ConflictingLinkEndReplay)
    );
}

#[test]
fn unbound_or_invalid_link_end_never_mutates_history() {
    let mut anonymous =
        ParticipantRecord::new_anonymous("participant_lifecycle", "tenant_lifecycle", 10_000)
            .unwrap();
    assert_eq!(
        anonymous.record_link_end("link_end_event_first", "link_end_evidence_first", 10_100),
        Err(AccountLinkError::NotLinked)
    );
    assert!(anonymous.link_end_history().is_empty());

    let mut participant = linked_participant();
    for (event_ref, evidence_ref, timestamp, expected) in [
        ("", "link_end_evidence_first", 10_200, AccountLinkError::InvalidReference),
        ("link_end_event_first", "12345", 10_200, AccountLinkError::InvalidReference),
        ("link_end_event_first", "link_end_evidence_first", 0, AccountLinkError::InvalidTimestamp),
        (
            "link_end_event_first",
            "link_end_evidence_first",
            10_099,
            AccountLinkError::NonMonotonicLifecycleTimestamp,
        ),
    ] {
        let links_before = participant.link_history().to_vec();
        let ends_before = participant.link_end_history().to_vec();
        assert_eq!(
            participant.record_link_end(event_ref, evidence_ref, timestamp),
            Err(expected)
        );
        assert_eq!(participant.link_history(), links_before.as_slice());
        assert_eq!(participant.link_end_history(), ends_before.as_slice());
        assert_eq!(participant.linked_subject_ref(), Some("keyverse_subject_first"));
    }
}

#[test]
fn relink_is_explicit_and_historical_replays_cannot_change_the_new_projection() {
    let mut participant = linked_participant();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();
    participant
        .link_account(
            "link_event_second",
            "keyverse_issuer_second",
            "keyverse_subject_second",
            "anonymous_proof_second",
            "authenticated_proof_second",
            10_300,
        )
        .unwrap();

    participant
        .link_account(
            "link_event_first",
            "keyverse_issuer_first",
            "keyverse_subject_first",
            "anonymous_proof_first",
            "authenticated_proof_first",
            10_100,
        )
        .unwrap();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();

    assert_eq!(participant.link_history().len(), 2);
    assert_eq!(participant.link_end_history().len(), 1);
    assert_eq!(participant.linked_subject_ref(), Some("keyverse_subject_second"));
}

#[test]
fn relink_must_not_precede_the_latest_link_end() {
    let mut participant = linked_participant();
    participant
        .record_link_end("link_end_event_first", "link_end_evidence_first", 10_200)
        .unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_second",
            "keyverse_issuer_second",
            "keyverse_subject_second",
            "anonymous_proof_second",
            "authenticated_proof_second",
            10_199,
        ),
        Err(AccountLinkError::NonMonotonicLifecycleTimestamp)
    );
    assert_eq!(participant.link_history().len(), 1);
    assert_eq!(participant.link_end_history().len(), 1);
    assert_eq!(participant.linked_subject_ref(), None);
}

#[test]
fn lifecycle_errors_expose_stable_safe_messages() {
    assert_eq!(
        AccountLinkError::NotLinked.to_string(),
        "participant has no current identity link to end"
    );
    assert_eq!(
        AccountLinkError::ConflictingLinkEndReplay.to_string(),
        "participant identity-link end event was replayed with conflicting evidence"
    );
    assert_eq!(
        AccountLinkError::NonMonotonicLifecycleTimestamp.to_string(),
        "participant identity-link lifecycle time must not move backwards"
    );
}
