//! Fail-closed contract for empty public research-release fixtures.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseLeakageError, RestrictedReleaseIdentities,
};

#[test]
fn empty_public_release_fixture_is_not_clean_release_evidence() {
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_inventory_evidence"],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    };

    assert_eq!(
        scan_public_release_fixture(&[], restricted),
        Err(PublicReleaseLeakageError::EmptyFixture)
    );
    assert_eq!(
        PublicReleaseLeakageError::EmptyFixture.to_string(),
        "supply at least one published column before treating a public release fixture scan as clean"
    );
}
