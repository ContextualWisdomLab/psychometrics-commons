//! Regression coverage for malformed restricted-identity inventories.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

#[test]
fn malformed_nonblank_restricted_identities_make_inventory_unusable() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "measurement_note",
        cell_values: &["public aggregate only"],
    }];

    for malformed in [
        " participant_seoul_clinic_one ",
        "participant_seoul_\u{200b}clinic_one",
        "participant_\u{0001}_seoul_clinic_one",
        "12345",
    ] {
        let restricted = RestrictedReleaseIdentities {
            operational_participant_refs: &[malformed],
            keyverse_subject_refs: &[],
            linkage_refs: &[],
            linkage_key_versions: &[],
        };

        assert_eq!(
            scan_public_release_fixture(&columns, restricted),
            Err(PublicReleaseLeakageError::IdentityInventoryUnavailable),
            "malformed restricted identity inventory entries cannot establish clean-release evidence"
        );
    }
}

#[test]
fn blank_placeholders_do_not_invalidate_an_otherwise_exact_inventory() {
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &["", "participant_seoul_clinic_one"],
        keyverse_subject_refs: &["   "],
        linkage_refs: &[],
        linkage_key_versions: &[],
    };
    let columns = [PublicReleaseFixtureColumn {
        column_name: "measurement_note",
        cell_values: &["public aggregate only"],
    }];

    assert_eq!(scan_public_release_fixture(&columns, restricted), Ok(()));
}
