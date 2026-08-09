//! Contract tests for anonymous-first participant identity and account linking.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

#[test]
fn participant_is_anonymous_by_default_and_keeps_product_identity_stable() {
    let participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 10_000).unwrap();

    assert_eq!(participant.participant_ref(), "participant_alpha");
    assert_eq!(participant.tenant_ref(), "tenant_alpha");
    assert_eq!(participant.created_at_unix_ms(), 10_000);
    assert_eq!(participant.linked_subject_ref(), None);
    assert_eq!(participant.link_event_ref(), None);
    assert_eq!(participant.anonymous_proof_ref(), None);
    assert_eq!(participant.authenticated_proof_ref(), None);
    assert_eq!(participant.linked_at_unix_ms(), None);
}

#[test]
fn linking_requires_both_anonymous_and_authenticated_proof() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 10_000).unwrap();

    participant
        .link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            10_100,
        )
        .unwrap();

    assert_eq!(participant.participant_ref(), "participant_alpha");
    assert_eq!(participant.tenant_ref(), "tenant_alpha");
    assert_eq!(
        participant.linked_subject_ref(),
        Some("keyverse_subject_alpha")
    );
    assert_eq!(participant.link_event_ref(), Some("link_event_alpha"));
    assert_eq!(
        participant.anonymous_proof_ref(),
        Some("anonymous_proof_alpha")
    );
    assert_eq!(
        participant.authenticated_proof_ref(),
        Some("authenticated_proof_alpha")
    );
    assert_eq!(participant.linked_at_unix_ms(), Some(10_100));
}

#[test]
fn exact_link_replay_is_idempotent_without_rewriting_product_identity() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 20_000).unwrap();

    for _ in 0..2 {
        participant
            .link_account(
                "link_event_alpha",
                "keyverse_subject_alpha",
                "anonymous_proof_alpha",
                "authenticated_proof_alpha",
                20_100,
            )
            .unwrap();
    }

    assert_eq!(participant.participant_ref(), "participant_alpha");
    assert_eq!(participant.linked_at_unix_ms(), Some(20_100));
}

#[test]
fn replay_with_changed_link_evidence_fails_closed() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 30_000).unwrap();
    participant
        .link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            30_100,
        )
        .unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_other",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            30_100,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_other",
            "authenticated_proof_alpha",
            30_100,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_other",
            30_100,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            30_101,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
}

#[test]
fn already_linked_participant_cannot_be_rebound_under_new_event_identity() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 40_000).unwrap();
    participant
        .link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            40_100,
        )
        .unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_beta",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            40_200,
        ),
        Err(AccountLinkError::AlreadyLinked)
    );
    assert_eq!(
        participant.link_account(
            "link_event_beta",
            "keyverse_subject_other",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            40_200,
        ),
        Err(AccountLinkError::AlreadyLinked)
    );
}

#[test]
fn account_link_time_is_server_monotonic() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 50_000).unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            0,
        ),
        Err(AccountLinkError::InvalidTimestamp)
    );
    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            49_999,
        ),
        Err(AccountLinkError::NonMonotonicTimestamp)
    );
    participant
        .link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            50_000,
        )
        .unwrap();
    assert_eq!(participant.linked_at_unix_ms(), Some(50_000));
}

#[test]
fn malformed_participant_and_link_references_fail_closed() {
    for invalid in ["", "   ", "12345"] {
        assert_eq!(
            ParticipantRecord::new_anonymous(invalid, "tenant_alpha", 60_000),
            Err(AccountLinkError::InvalidReference)
        );
        assert_eq!(
            ParticipantRecord::new_anonymous("participant_alpha", invalid, 60_000),
            Err(AccountLinkError::InvalidReference)
        );
    }
    assert_eq!(
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 0),
        Err(AccountLinkError::InvalidTimestamp)
    );

    let invalid_cases = [
        (
            "12345",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
        ),
        (
            "link_event_alpha",
            "12345",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
        ),
        (
            "link_event_alpha",
            "keyverse_subject_alpha",
            "12345",
            "authenticated_proof_alpha",
        ),
        (
            "link_event_alpha",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "12345",
        ),
    ];
    for (event_ref, subject_ref, anonymous_proof_ref, authenticated_proof_ref) in invalid_cases {
        let mut participant =
            ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 60_000).unwrap();
        assert_eq!(
            participant.link_account(
                event_ref,
                subject_ref,
                anonymous_proof_ref,
                authenticated_proof_ref,
                60_100,
            ),
            Err(AccountLinkError::InvalidReference)
        );
    }
}

#[test]
fn account_link_errors_have_stable_safe_messages() {
    let cases = [
        (
            AccountLinkError::InvalidReference,
            "participant account-link references must be opaque non-numeric values",
        ),
        (
            AccountLinkError::InvalidTimestamp,
            "participant account-link timestamps must be greater than zero",
        ),
        (
            AccountLinkError::NonMonotonicTimestamp,
            "participant account-link time must not precede participant creation",
        ),
        (
            AccountLinkError::ConflictingReplay,
            "participant account-link event was replayed with conflicting evidence",
        ),
        (
            AccountLinkError::AlreadyLinked,
            "participant is already linked and cannot be rebound in place",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}
