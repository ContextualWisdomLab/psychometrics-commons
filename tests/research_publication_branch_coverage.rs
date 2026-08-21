//! Focused branch coverage for public research fixture privacy scanning.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, RestrictedReleaseIdentities,
};

#[test]
fn benign_empty_cells_are_allowed_when_restricted_inventory_is_present() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "research_program_ref",
        cell_values: &["", "   ", "research_program_alpha"],
    }];
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_inventory_evidence"],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    };

    assert_eq!(scan_public_release_fixture(&columns, restricted), Ok(()));
}

#[test]
fn blank_inventory_entries_are_ignored_when_effective_evidence_is_also_present() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "theta_estimate",
        cell_values: &["0.42"],
    }];
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &["   ", "participant_inventory_evidence"],
        keyverse_subject_refs: &[""],
        linkage_refs: &["  "],
        linkage_key_versions: &["linkage_key_inventory_evidence"],
    };

    assert_eq!(scan_public_release_fixture(&columns, restricted), Ok(()));
}

#[test]
fn digit_to_camel_case_transition_stays_a_benign_nonidentity_column() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "wave2ParticipantScore",
        cell_values: &["0.42"],
    }];
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_inventory_evidence"],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    };

    assert_eq!(scan_public_release_fixture(&columns, restricted), Ok(()));
}
