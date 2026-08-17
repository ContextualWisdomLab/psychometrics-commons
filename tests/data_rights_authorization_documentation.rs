const AUTHORIZATION_ARCHITECTURE: &str =
    include_str!("../docs/architecture/DATA_RIGHTS_AUTHORIZATION.md");

#[test]
fn stored_data_rights_authorization_boundary_stays_documented() {
    for required in [
        "PARTIAL",
        "PRD (Product Requirements Document)",
        "TRD (Technical Requirements Document)",
        "ADR (Architecture Decision Record)",
        "GA (General Availability)",
        "Keyverse, the external identity and federation service",
        "`ResourceScope`, the product-owned authorization target",
        "`ManageOwnDataRights`, the permission",
        "DataRightsRequest",
        "src/data_rights_authorization.rs::authorize_data_rights_request",
        "stored request",
        "cross-tenant",
        "required binding is missing, malformed, cross-tenant, or owned by another participant",
        "Hosted HTTP/repository adapters must call this stored-record composition",
        "transport-level negative tests remain required before GA",
        "no superseding ADR is required",
    ] {
        assert!(
            AUTHORIZATION_ARCHITECTURE.contains(required),
            "data-rights authorization architecture must retain {required:?}"
        );
    }
}
