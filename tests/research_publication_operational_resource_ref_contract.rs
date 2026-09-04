//! Regression coverage for operational product-resource identifiers in public releases.

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
fn operational_product_resource_references_fail_closed() {
    for column_name in [
        "assessment_session_ref",
        "session_ref",
        "result_ref",
        "response_ref",
        "item_delivery_ref",
        "scoring_request_ref",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_operational_resource_ref"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} links a research row back to an operational product resource and must not enter a public release"
        );
    }
}

#[test]
fn research_specific_nonoperational_descriptors_remain_publishable() {
    for column_name in ["research_wave", "research_measurement_window"] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["wave_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Ok(()),
            "{column_name} is research metadata rather than an operational product-resource reference"
        );
    }
}
