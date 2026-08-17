//! Regression coverage for fail-closed opaque public-reference spelling.

use psychometrics_commons_runtime::anonymous_session::{
    AnonymousSessionContext, AnonymousSessionContextError,
};
use psychometrics_commons_runtime::authorization::{AuthorizationContext, AuthorizationError};
use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::SessionState;

const VALID_PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn whitespace_padded_public_references_are_rejected_at_every_constructor_slot() {
    for invalid_reference in [
        " tenant_ref",
        "tenant_ref ",
        "\ttenant_ref",
        "tenant_ref\n",
        "\u{00a0}tenant_ref",
        "tenant_ref\u{2003}",
    ] {
        for field_index in 0..4 {
            let mut references = [
                "tenant_ref",
                "participant_ref",
                "session_ref",
                "authorization_evidence_ref",
            ];
            references[field_index] = invalid_reference;
            assert_eq!(
                AnonymousSessionContext::new(
                    references[0],
                    references[1],
                    references[2],
                    references[3],
                    10_000,
                ),
                Err(AnonymousSessionContextError::InvalidReference),
                "anonymous-session field {field_index} must reject non-canonical reference spelling {invalid_reference:?}",
            );
        }

        for field_index in 0..4 {
            let mut references = [
                "request_ref",
                "tenant_ref",
                "participant_ref",
                "account_data_scope",
            ];
            references[field_index] = invalid_reference;
            assert_eq!(
                DataRightsRequest::new(
                    references[0],
                    references[1],
                    references[2],
                    DataRightsRequestKind::Export,
                    references[3],
                    1_000,
                ),
                Err(DataRightsError::InvalidReference),
                "data-rights field {field_index} must reject non-canonical reference spelling {invalid_reference:?}",
            );
        }

        for field_index in 0..3 {
            let mut references = ["tenant_ref", "subject_ref", "participant_ref"];
            references[field_index] = invalid_reference;
            assert_eq!(
                AuthorizationContext::new(references[0], references[1], Some(references[2]), &[]),
                Err(AuthorizationError::InvalidReference),
                "authorization field {field_index} must reject non-canonical reference spelling {invalid_reference:?}",
            );
        }

        assert_eq!(
            ResponseLedger::new(invalid_reference),
            Err(WriteError::InvalidReference),
            "response session reference must reject non-canonical spelling {invalid_reference:?}",
        );

        let mut response_ledger = ResponseLedger::new("session_ref").unwrap();
        for field_index in 0..3 {
            let mut references = ["server_event_ref", "client_event_ref", "item_version_ref"];
            references[field_index] = invalid_reference;
            assert_eq!(
                response_ledger.record(
                    SessionState::Active,
                    ResponseWrite {
                        server_event_ref: references[0],
                        client_event_ref: references[1],
                        item_version_ref: references[2],
                        payload_digest: VALID_PAYLOAD_DIGEST,
                    },
                ),
                Err(WriteError::InvalidReference),
                "response event field {field_index} must reject non-canonical spelling {invalid_reference:?}",
            );
        }
        assert_eq!(
            response_ledger.freeze_as(SessionState::Completed, invalid_reference),
            Err(WriteError::InvalidReference),
            "response snapshot reference must reject non-canonical spelling {invalid_reference:?}",
        );
    }
}

#[test]
fn embedded_control_characters_are_rejected_at_public_reference_boundaries() {
    for invalid_reference in [
        "tenant\nref",
        "tenant\rref",
        "tenant\tref",
        "tenant\u{0000}ref",
        "tenant\u{001b}ref",
        "tenant\u{007f}ref",
    ] {
        assert_eq!(
            AnonymousSessionContext::new(
                invalid_reference,
                "participant_ref",
                "session_ref",
                "authorization_evidence_ref",
                10_000,
            ),
            Err(AnonymousSessionContextError::InvalidReference),
            "anonymous-session references must reject embedded control characters {invalid_reference:?}",
        );
        assert_eq!(
            DataRightsRequest::new(
                "request_ref",
                invalid_reference,
                "participant_ref",
                DataRightsRequestKind::Export,
                "account_data_scope",
                1_000,
            ),
            Err(DataRightsError::InvalidReference),
            "data-rights references must reject embedded control characters {invalid_reference:?}",
        );
        assert_eq!(
            AuthorizationContext::new(invalid_reference, "subject_ref", Some("participant_ref"), &[]),
            Err(AuthorizationError::InvalidReference),
            "authorization references must reject embedded control characters {invalid_reference:?}",
        );
        assert_eq!(
            ResponseLedger::new(invalid_reference),
            Err(WriteError::InvalidReference),
            "response references must reject embedded control characters {invalid_reference:?}",
        );
    }
}

#[test]
fn default_ignorable_and_bidirectional_format_characters_are_rejected() {
    // Unicode UTS #39 version 17.0.0 marks Default_Ignorable identifiers as Restricted.
    // Cover representatives beyond bidi controls so variation selectors, fillers, tags,
    // shorthand/music controls, and joiners cannot create byte-distinct invisible aliases.
    for invalid_reference in [
        "tenant\u{00ad}ref",  // SOFT HYPHEN
        "tenant\u{034f}ref",  // COMBINING GRAPHEME JOINER
        "tenant\u{061c}ref",  // ARABIC LETTER MARK
        "tenant\u{115f}ref",  // HANGUL CHOSEONG FILLER
        "tenant\u{17b4}ref",  // KHMER VOWEL INHERENT AQ
        "tenant\u{180b}ref",  // MONGOLIAN FREE VARIATION SELECTOR ONE
        "tenant\u{200b}ref",  // ZERO WIDTH SPACE
        "tenant\u{200e}ref",  // LEFT-TO-RIGHT MARK
        "tenant\u{202e}ref",  // RIGHT-TO-LEFT OVERRIDE
        "tenant\u{2066}ref",  // LEFT-TO-RIGHT ISOLATE
        "tenant\u{2060}ref",  // WORD JOINER
        "tenant\u{3164}ref",  // HANGUL FILLER
        "tenant\u{fe0f}ref",  // VARIATION SELECTOR-16
        "tenant\u{feff}ref",  // ZERO WIDTH NO-BREAK SPACE / BOM
        "tenant\u{ffa0}ref",  // HALFWIDTH HANGUL FILLER
        "tenant\u{1bca0}ref", // SHORTHAND FORMAT LETTER OVERLAP
        "tenant\u{1d173}ref", // MUSICAL SYMBOL BEGIN BEAM
        "tenant\u{fff0}ref",  // <not a character> default-ignorable
        "tenant\u{e0001}ref", // LANGUAGE TAG
        "tenant\u{e0020}ref", // TAG SPACE
        "tenant\u{e0100}ref", // VARIATION SELECTOR-17
    ] {
        assert_eq!(
            AnonymousSessionContext::new(
                invalid_reference,
                "participant_ref",
                "session_ref",
                "authorization_evidence_ref",
                10_000,
            ),
            Err(AnonymousSessionContextError::InvalidReference),
            "anonymous-session references must reject default-ignorable formatting {invalid_reference:?}",
        );
        assert_eq!(
            DataRightsRequest::new(
                "request_ref",
                invalid_reference,
                "participant_ref",
                DataRightsRequestKind::Export,
                "account_data_scope",
                1_000,
            ),
            Err(DataRightsError::InvalidReference),
            "data-rights references must reject default-ignorable formatting {invalid_reference:?}",
        );
        assert_eq!(
            AuthorizationContext::new(invalid_reference, "subject_ref", Some("participant_ref"), &[]),
            Err(AuthorizationError::InvalidReference),
            "authorization references must reject default-ignorable formatting {invalid_reference:?}",
        );
        assert_eq!(
            ResponseLedger::new(invalid_reference),
            Err(WriteError::InvalidReference),
            "response references must reject default-ignorable formatting {invalid_reference:?}",
        );
    }
}

#[test]
fn canonical_opaque_public_references_remain_accepted() {
    let anonymous = AnonymousSessionContext::new(
        "tenant_ref",
        "participant_ref",
        "session_ref",
        "authorization_evidence_ref",
        10_000,
    )
    .unwrap();
    assert_eq!(anonymous.tenant_ref(), "tenant_ref");

    let request = DataRightsRequest::new(
        "request_ref",
        "tenant_ref",
        "participant_ref",
        DataRightsRequestKind::Export,
        "account_data_scope",
        1_000,
    )
    .unwrap();
    assert_eq!(request.tenant_ref(), "tenant_ref");

    let authorization =
        AuthorizationContext::new("tenant_ref", "subject_ref", Some("participant_ref"), &[])
            .unwrap();
    assert_eq!(authorization.tenant_ref(), "tenant_ref");
    assert_eq!(authorization.subject_ref(), "subject_ref");
    assert_eq!(authorization.participant_ref(), Some("participant_ref"));

    let mut response_ledger = ResponseLedger::new("session_ref").unwrap();
    let response = response_ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "server_event_ref",
                client_event_ref: "client_event_ref",
                item_version_ref: "item_version_ref",
                payload_digest: VALID_PAYLOAD_DIGEST,
            },
        )
        .unwrap();
    assert_eq!(response.server_event_ref(), "server_event_ref");
    let snapshot = response_ledger
        .freeze_as(SessionState::Completed, "snapshot_ref")
        .unwrap();
    assert_eq!(snapshot.snapshot_ref(), Some("snapshot_ref"));
}
