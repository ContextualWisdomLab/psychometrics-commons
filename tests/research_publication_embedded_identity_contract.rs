//! Regression coverage for restricted identities embedded in otherwise flat public cells.

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
fn embedded_restricted_identities_fail_closed_inside_flat_public_cells() {
    for (cell, expected) in [
        (
            "completed by participant_seoul_clinic_one at baseline",
            PublicReleaseLeakageError::OperationalParticipant,
        ),
        (
            "source=keyverse_subject_seoul_clinic_one; imported",
            PublicReleaseLeakageError::KeyverseSubject,
        ),
        (
            "legacy linkage_seoul_clinic_one retained in note",
            PublicReleaseLeakageError::RestrictedLinkage,
        ),
        (
            "key version linkage_key_version_2026_q3 was used",
            PublicReleaseLeakageError::RestrictedLinkage,
        ),
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name: "measurement_note",
            cell_values: &[cell],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(expected),
            "restricted identities embedded in flat text must not be publishable"
        );
    }
}

#[test]
fn nearby_nonmatching_public_text_remains_allowed() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "measurement_note",
        cell_values: &["completed by participant_seoul_clinic_two at baseline"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, restricted_identities()),
        Ok(())
    );
}
