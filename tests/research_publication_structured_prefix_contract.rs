//! Privacy regression for structured public-release cells hidden behind invisible prefixes.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

fn restricted_identities() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_restricted_one"],
        keyverse_subject_refs: &["keyverse_restricted_one"],
        linkage_refs: &["linkage_restricted_one"],
        linkage_key_versions: &["linkage_key_version_restricted_one"],
    }
}

#[test]
fn invisible_prefixes_cannot_disguise_structured_public_release_cells() {
    for value in [
        "\u{feff}{\"participant_ref\":\"participant_restricted_one\"}",
        "\u{200b}[\"participant_restricted_one\"]",
        "\u{2060} {\"subject\":\"keyverse_restricted_one\"}",
        "\u{e0001}\t[\"linkage_restricted_one\"]",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name: "research_note",
            cell_values: &[value],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::StructuredValueUnsupported),
            "structured release cell hidden by an invisible prefix must fail closed: {value:?}"
        );
    }
}
