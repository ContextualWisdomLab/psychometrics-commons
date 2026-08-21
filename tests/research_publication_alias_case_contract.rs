//! Adversarial alias cases for public research-release fixture identity columns.

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
fn uppercase_runs_cannot_bypass_identity_column_denylist() {
    for column_name in [
        "assessmentPARTICIPANTRef",
        "pseudonymKEYVersion",
        "identitySUBJECTRef",
        "linkedSUBJECTRef",
        "ASSESSMENTPARTICIPANTREF",
        "PSEUDONYMKEYVERSION",
        "IDENTITYSUBJECTREF",
        "LINKEDSUBJECTREF",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not bypass the public-release identity-column denylist"
        );
    }
}

#[test]
fn unavailable_identity_inventory_error_is_safe_and_actionable() {
    let message = PublicReleaseLeakageError::IdentityInventoryUnavailable.to_string();

    assert_eq!(
        message,
        "supply an authorized restricted-identity inventory before packaging the public release fixture"
    );
    assert!(!message.contains("participant_seoul_clinic_one"));
    assert!(!message.contains("keyverse_subject_seoul_clinic_one"));
    assert!(!message.contains("linkage_seoul_clinic_one"));
}
