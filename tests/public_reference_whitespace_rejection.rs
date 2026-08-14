//! Regression coverage for fail-closed opaque public-reference spelling.

use psychometrics_commons_runtime::anonymous_session::{
    AnonymousSessionContext, AnonymousSessionContextError,
};
use psychometrics_commons_runtime::data_rights::{
    DataRightsError, DataRightsRequest, DataRightsRequestKind,
};

#[test]
fn whitespace_padded_public_references_are_rejected_at_construction() {
    for invalid_reference in [" tenant_ref", "tenant_ref ", "\ttenant_ref", "tenant_ref\n"] {
        assert_eq!(
            AnonymousSessionContext::new(
                invalid_reference,
                "participant_ref",
                "session_ref",
                "authorization_evidence_ref",
                10_000,
            ),
            Err(AnonymousSessionContextError::InvalidReference),
            "anonymous-session construction must reject non-canonical reference spelling {invalid_reference:?}",
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
            "data-rights construction must reject non-canonical reference spelling {invalid_reference:?}",
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
}
