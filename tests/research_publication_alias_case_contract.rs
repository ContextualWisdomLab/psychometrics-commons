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
fn prefixed_etl_aliases_cannot_hide_restricted_identity_columns() {
    for column_name in [
        "export_assessment_participant_ref",
        "sourcePseudonymKeyVersion",
        "stagingIDENTITYSUBJECTREF",
        "warehouseLinkedSubjectRef",
        "etlSubjectRef",
        "customerParticipantRef",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not hide a restricted identity field behind an ETL prefix"
        );
    }
}

#[test]
fn prefixed_research_participant_namespace_remains_public() {
    for column_name in ["public_research_participant_ref", "exportResearchParticipantRef"] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Ok(()),
            "{column_name} must remain in the explicit public research identity namespace"
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
