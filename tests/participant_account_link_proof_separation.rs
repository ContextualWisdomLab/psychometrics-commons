//! Regression test for independent account-link proof references.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

#[test]
fn anonymous_and_authenticated_control_must_use_distinct_proof_references() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 10_000).unwrap();

    assert_eq!(
        participant.link_account(
            "link_event_alpha",
            "keyverse_subject_alpha",
            "shared_proof_alpha",
            "shared_proof_alpha",
            10_100,
        ),
        Err(AccountLinkError::ProofReferenceReuse)
    );
    assert_eq!(participant.linked_subject_ref(), None);
    assert_eq!(
        AccountLinkError::ProofReferenceReuse.to_string(),
        "anonymous and authenticated account-link proofs must use distinct references"
    );
}
