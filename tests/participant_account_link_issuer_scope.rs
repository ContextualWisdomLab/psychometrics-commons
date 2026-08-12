//! Regression tests for issuer-scoped external identity on participant account links.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

#[test]
fn account_link_pins_identity_issuer_with_provider_scoped_subject() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 10_000).unwrap();

    participant
        .link_account(
            "link_event_alpha",
            "keyverse_issuer_prod",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            10_100,
        )
        .unwrap();

    assert_eq!(
        participant.linked_issuer_ref(),
        Some("keyverse_issuer_prod")
    );
    assert_eq!(
        participant.linked_subject_ref(),
        Some("keyverse_subject_alpha")
    );
}

#[test]
fn replay_cannot_substitute_identity_issuer_under_same_event() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 20_000).unwrap();
    participant
        .link_account(
            "link_event_alpha",
            "keyverse_issuer_prod",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            20_100,
        )
        .unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_issuer_other",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            20_100,
        ),
        Err(AccountLinkError::ConflictingReplay)
    );
    assert_eq!(
        participant.linked_issuer_ref(),
        Some("keyverse_issuer_prod")
    );
}

#[test]
fn malformed_identity_issuer_fails_closed_without_linking() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 30_000).unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "12345",
            "keyverse_subject_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            30_100,
        ),
        Err(AccountLinkError::InvalidReference)
    );
    assert_eq!(participant.linked_issuer_ref(), None);
    assert_eq!(participant.linked_subject_ref(), None);
}
