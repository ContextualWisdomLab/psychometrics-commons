//! Public-reference tests for numeric-looking identifiers.
//!
//! References made only from numeric characters and their punctuation or signs are rejected.
//! References that combine letters with numbers remain valid opaque identifiers and keep their
//! original spelling.

use psychometrics_commons_runtime::participant::{AccountLinkError, ParticipantRecord};

#[test]
fn numeric_literal_spellings_fail_closed_across_supported_unicode_separators() {
    let numeric_like_refs = [
        "123",
        "+123",
        "-123",
        "1.23",
        "1,234",
        "1e3",
        "1E3",
        "١٢٣",
        "١٢\u{066B}٣",
        "١\u{066C}٢٣٤",
        "１２３",
        "１２\u{FF0E}３",
        "１\u{FF0C}２３４",
    ];

    for invalid_ref in numeric_like_refs {
        assert_eq!(
            ParticipantRecord::new_anonymous(invalid_ref, "tenant_alpha", 1),
            Err(AccountLinkError::InvalidReference),
            "numeric-like participant reference should fail closed: {invalid_ref:?}",
        );
        assert_eq!(
            ParticipantRecord::new_anonymous("participant_alpha", invalid_ref, 1),
            Err(AccountLinkError::InvalidReference),
            "numeric-like tenant reference should fail closed: {invalid_ref:?}",
        );
    }
}

#[test]
fn numeric_punctuation_remains_valid_inside_opaque_mixed_references() {
    for valid_ref in [
        "participant-1",
        "participant.1",
        "participant,1",
        "participant+1",
        "participant_e1",
        "participantＥ1",
    ] {
        let participant = ParticipantRecord::new_anonymous(valid_ref, "tenant_alpha", 1)
            .expect("mixed opaque participant references remain valid");
        assert_eq!(participant.participant_ref(), valid_ref);

        let tenant = ParticipantRecord::new_anonymous("participant_alpha", valid_ref, 1)
            .expect("mixed opaque tenant references remain valid");
        assert_eq!(tenant.tenant_ref(), valid_ref);
    }
}
