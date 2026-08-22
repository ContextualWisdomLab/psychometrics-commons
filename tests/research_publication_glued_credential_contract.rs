//! Regression coverage for separator-free credential aliases in public releases.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

fn restricted_identities() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_seoul_clinic_one"],
        keyverse_subject_refs: &["keyverse_subject_seoul_clinic_one"],
        linkage_refs: &["linkage_seoul_clinic_one"],
        linkage_key_versions: &["linkage_key_version_2026_q3"],
    }
}

#[test]
fn glued_credential_markers_fail_closed() {
    for column_name in [
        "usersecretvalue",
        "userpasswordhash",
        "sessiontokenvalue",
        "apicredentialvalue",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["sensitive_material_not_in_identity_inventory"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not expose credential material in a public release"
        );
    }
}
