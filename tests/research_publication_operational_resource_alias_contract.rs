//! Adversarial aliases for operational hosted-resource identifiers in public releases.

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
fn operational_resource_aliases_cannot_bypass_column_normalization() {
    for column_name in [
        "exportAssessmentSessionRef",
        "warehouse-session-ref",
        "public.result.ref",
        "staging/response/ref",
        "itemDeliveryRefBackup",
        "source_scoring_request_ref_v2",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_operational_resource_ref"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not hide an operational product-resource reference"
        );
    }
}
