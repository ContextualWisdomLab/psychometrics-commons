const AUTHORIZATION_ARCHITECTURE: &str =
    include_str!("../docs/architecture/DATA_RIGHTS_AUTHORIZATION.md");

#[test]
fn stored_data_rights_authorization_boundary_stays_documented() {
    for required in [
        "PARTIAL",
        "TRD §11",
        "DataRightsRequest",
        "src/data_rights_authorization.rs::authorize_data_rights_request",
        "ManageOwnDataRights",
        "stored request",
        "cross-tenant",
        "no superseding ADR is required",
    ] {
        assert!(
            AUTHORIZATION_ARCHITECTURE.contains(required),
            "data-rights authorization architecture must retain {required:?}"
        );
    }
}
