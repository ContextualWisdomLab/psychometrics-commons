//! Regression coverage for credential-like public-release column names.
//!
//! Credential or secret markers must remain forbidden when they appear at the
//! beginning of a compact column name, not only when separated by punctuation.

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
fn leading_credential_markers_fail_closed_without_separator_boundaries() {
    for column_name in [
        "SECRETKEY",
        "PASSWORDHASH",
        "TOKENID",
        "CREDENTIALDIGEST",
        "secretKey",
        "passwordHash",
        "tokenId",
        "credentialDigest",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["sensitive_material_not_in_identity_inventory"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not expose credential or secret material in a public release"
        );
    }
}
