//! Exact-spelling regressions for product-owned participant and account-link identity.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

const PADDED_REFERENCES: [&str; 5] = [
    " participant_alpha",
    "participant_alpha\u{00a0}",
    "\u{2003}participant_alpha",
    "participant_alpha\u{202f}",
    "participant_alpha\u{3000}",
];

#[test]
fn anonymous_participant_creation_rejects_padded_identity_aliases() {
    for padded in PADDED_REFERENCES {
        assert_eq!(
            ParticipantRecord::new_anonymous(padded, "tenant_alpha", 10_000),
            Err(AccountLinkError::InvalidReference),
        );
        assert_eq!(
            ParticipantRecord::new_anonymous("participant_alpha", padded, 10_000),
            Err(AccountLinkError::InvalidReference),
        );
    }
}

#[test]
fn account_link_lifecycle_rejects_padded_evidence_aliases() {
    for position in 0..5 {
        let mut participant =
            ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 20_000)
                .expect("exact participant identity is valid");
        let mut refs = [
            "link_event_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
        ];
        refs[position] = PADDED_REFERENCES[position];

        assert_eq!(
            participant.link_account(refs[0], refs[1], refs[2], refs[3], refs[4], 20_100),
            Err(AccountLinkError::InvalidReference),
        );
    }

    let mut participant =
        ParticipantRecord::new_anonymous("participant_alpha", "tenant_alpha", 30_000)
            .expect("exact participant identity is valid");
    participant
        .link_account(
            "link_event_alpha",
            "issuer_keyverse_prod",
            "subject_account_alpha",
            "anonymous_proof_alpha",
            "authenticated_proof_alpha",
            30_100,
        )
        .expect("exact link evidence is valid");

    assert_eq!(
        participant.record_link_end(" link_end_event_alpha", "unlink_evidence_alpha", 30_200),
        Err(AccountLinkError::InvalidReference),
    );
    assert_eq!(
        participant.record_link_end("link_end_event_alpha", "unlink_evidence_alpha\u{3000}", 30_200),
        Err(AccountLinkError::InvalidReference),
    );
}

#[test]
fn participant_identity_preserves_visible_multilingual_references_exactly() {
    let mut participant =
        ParticipantRecord::new_anonymous("participant_서울", "tenant_대한민국", 40_000)
            .expect("visible multilingual references are valid opaque identity");
    participant
        .link_account(
            "link_event_연결",
            "issuer_키버스",
            "subject_사용자",
            "anonymous_proof_익명",
            "authenticated_proof_인증",
            40_100,
        )
        .expect("visible multilingual link evidence is valid");

    assert_eq!(participant.participant_ref(), "participant_서울");
    assert_eq!(participant.tenant_ref(), "tenant_대한민국");
    assert_eq!(participant.link_event_ref(), Some("link_event_연결"));
    assert_eq!(participant.linked_issuer_ref(), Some("issuer_키버스"));
    assert_eq!(participant.linked_subject_ref(), Some("subject_사용자"));
    assert_eq!(participant.anonymous_proof_ref(), Some("anonymous_proof_익명"));
    assert_eq!(
        participant.authenticated_proof_ref(),
        Some("authenticated_proof_인증")
    );
}