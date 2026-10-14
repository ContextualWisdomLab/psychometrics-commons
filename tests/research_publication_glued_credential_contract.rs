//! Public-release credential checks reject glued aliases that hide a sensitive marker.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

fn restricted_identity_inventory() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_inventory_evidence"],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    }
}

#[test]
fn glued_credential_markers_fail_closed_before_public_packaging() {
    for column_name in [
        "usersecretvalue",
        "sessiontokenvalue",
        "accountpasswordhash",
        "servicecredentialblob",
        "sessionkey",
        "encryptionkey",
        "signingkeys",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["public_value"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identity_inventory()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must fail closed when a credential marker is glued into the column name"
        );
    }
}
