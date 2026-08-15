//! Regression coverage for fail-closed opaque public-reference spelling.

use psychometrics_commons_runtime::anonymous_session::{
    AnonymousSessionContext, AnonymousSessionContextError,
};
use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError,
};
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

    let authorization = AuthorizationContext::new(
        "tenant_ref",
        "subject_ref",
        Some("participant_ref"),
        &[],
    )
    .unwrap();
    assert_eq!(authorization.tenant_ref(), "tenant_ref");
    assert_eq!(authorization.subject_ref(), "subject_ref");
    assert_eq!(authorization.participant_ref(), Some("participant_ref"));
}
