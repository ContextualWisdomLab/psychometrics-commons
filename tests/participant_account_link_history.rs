//! Regression coverage for append-only participant account-link audit history.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

fn anonymous_participant() -> ParticipantRecord {
    ParticipantRecord::new_anonymous("participant_history", "tenant_history", 10_000).unwrap()
}

#[test]
fn account_link_appends_exact_audit_evidence_without_rewriting_participant_identity() {
    let mut participant = anonymous_participant();
    assert!(participant.link_history().is_empty());

    participant
        .link_account(
            "link_event_history",
            "keyverse_issuer_history",
            "keyverse_subject_history",
            "anonymous_proof_history",
            "authenticated_proof_history",
            10_100,
        )
        .unwrap();

    assert_eq!(participant.participant_ref(), "participant_history");
    assert_eq!(participant.link_history().len(), 1);
    let event = &participant.link_history()[0];
    assert_eq!(event.link_event_ref(), "link_event_history");
    assert_eq!(event.issuer_ref(), "keyverse_issuer_history");
    assert_eq!(event.subject_ref(), "keyverse_subject_history");
    assert_eq!(event.anonymous_proof_ref(), "anonymous_proof_history");
    assert_eq!(
        event.authenticated_proof_ref(),
        "authenticated_proof_history"
    );
    assert_eq!(event.linked_at_unix_ms(), 10_100);
}

#[test]
fn exact_replay_is_idempotent_and_does_not_duplicate_history() {
    let mut participant = anonymous_participant();

    for _ in 0..2 {
        participant
            .link_account(
                "link_event_history",
                "keyverse_issuer_history",
                "keyverse_subject_history",
                "anonymous_proof_history",
                "authenticated_proof_history",
                10_100,
            )
            .unwrap();
    }

    assert_eq!(participant.link_history().len(), 1);
    assert_eq!(participant.link_history()[0].link_event_ref(), "link_event_history");
}

#[test]
fn rejected_replay_or_rebinding_cannot_append_or_mutate_history() {
    let mut participant = anonymous_participant();
    participant
        .link_account(
            "link_event_history",
            "keyverse_issuer_history",
            "keyverse_subject_history",
            "anonymous_proof_history",
            "authenticated_proof_history",
            10_100,
        )
        .unwrap();
    let original_history = participant.link_history().to_vec();

    assert_eq!(
        participant.link_account(
            "link_event_history",
            "keyverse_issuer_history",
            "keyverse_subject_other",
            "anonymous_proof_history",
            "authenticated_proof_history",
            10_100,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
    assert_eq!(participant.link_history(), original_history.as_slice());

    assert_eq!(
        participant.link_account(
            "link_event_other",
            "keyverse_issuer_other",
            "keyverse_subject_other",
            "anonymous_proof_other",
            "authenticated_proof_other",
            10_200,
        ),
        Err(AccountLinkError::AlreadyLinked)
    );
    assert_eq!(participant.link_history(), original_history.as_slice());
}
