//! Contract coverage for opaque public-reference numeric-literal rejection.
//!
//! Public references may contain digits when they are part of an opaque identifier, but a
//! spelling made only from Unicode numeric code points plus numeric-literal punctuation/signs
//! must fail closed. This exercises every separator/sign branch in the shared reference boundary
//! through a public product constructor rather than reaching into the private validator.

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
    }
}
