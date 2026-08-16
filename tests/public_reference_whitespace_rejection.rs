//! Regression coverage for fail-closed opaque public-reference spelling.

use psychometrics_commons_runtime::anonymous_session::{
    AnonymousSessionContext, AnonymousSessionContextError,
};
use psychometrics_commons_runtime::authorization::{AuthorizationContext, AuthorizationError};
use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind,
};

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
    }
}

#[test]
fn invisible_and_bidirectional_format_characters_are_rejected() {
    // Unicode UTS #39 classifies default-ignorable characters as restricted for security
    // identifiers. These examples are not `char::is_control`, so they protect the distinct
    // spoofing/log-reordering boundary that ordinary C0/C1 control tests cannot exercise.
    for invalid_reference in [
        "tenant\u{200b}ref", // ZERO WIDTH SPACE
        "tenant\u{200e}ref", // LEFT-TO-RIGHT MARK
        "tenant\u{202e}ref", // RIGHT-TO-LEFT OVERRIDE
        "tenant\u{2066}ref", // LEFT-TO-RIGHT ISOLATE
        "tenant\u{2060}ref", // WORD JOINER
        "tenant\u{feff}ref", // ZERO WIDTH NO-BREAK SPACE / BOM
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
            "anonymous-session references must reject invisible or directional formatting {invalid_reference:?}",
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
            "data-rights references must reject invisible or directional formatting {invalid_reference:?}",
        );
        assert_eq!(
            AuthorizationContext::new(invalid_reference, "subject_ref", Some("participant_ref"), &[]),
            Err(AuthorizationError::InvalidReference),
            "authorization references must reject invisible or directional formatting {invalid_reference:?}",
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
}
