//! Regression coverage for visually obfuscated restricted identities in public cells.

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
fn default_ignorable_characters_cannot_obfuscate_restricted_identity_leakage() {
    for (cell, expected) in [
        (
            "participant_seoul_\u{200b}clinic_one",
            PublicReleaseLeakageError::OperationalParticipant,
        ),
        (
            "keyverse_subject_\u{2060}seoul_clinic_one",
            PublicReleaseLeakageError::KeyverseSubject,
        ),
        (
            "linkage_seoul_clinic_\u{fe0f}one",
            PublicReleaseLeakageError::RestrictedLinkage,
        ),
        (
            "linkage_key_version_2026_\u{e0001}q3",
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
            "default-ignorable characters must not make a restricted identity publishable"
        );
    }
}

#[test]
fn removing_default_ignorables_does_not_create_an_unrelated_identity_match() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "measurement_note",
        cell_values: &["participant_seoul_\u{200b}clinic_two"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, restricted_identities()),
        Ok(())
    );
}
