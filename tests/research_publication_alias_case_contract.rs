//! Adversarial alias and structured-value cases for public research-release fixtures.

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
fn suffixed_aliases_cannot_hide_restricted_identity_columns() {
    for column_name in [
        "participant_ref_v2",
        "subject_reference",
        "linked_subject_ref_backup",
        "linkage-key-version-old",
        "pseudonym key version legacy",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not hide a restricted identity field behind a suffix"
        );
    }
}

#[test]
fn separator_aliases_cannot_hide_restricted_identity_columns() {
    for column_name in [
        "export-participant-ref",
        "warehouse.subject_ref",
        "customer participant id",
        "source/linkage/key/version",
        "staging:pseudonym:key:version",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not hide a restricted identity field with alternate separators"
        );
    }
}

#[test]
fn pseudonym_linkage_key_aliases_cannot_enter_public_release() {
    for column_name in [
        "pseudonym_key",
        "export_pseudonym_key",
        "pseudonym_key_backup",
        "pseudonym-key",
        "pseudonym.key",
        "pseudonym key",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not expose a restricted pseudonym linkage key"
        );
    }
}

#[test]
fn non_ascii_column_aliases_fail_closed() {
    for column_name in [
        "аccess_token",
        "access_tоken",
        "participant_rеf",
        "research_participant_rеf",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not bypass the ASCII public-release column grammar"
        );
    }
}

#[test]
fn credential_and_internal_location_columns_cannot_enter_public_release() {
    for column_name in [
        "service_access_token",
        "oauth_refresh_token",
        "oidc_client_secret",
        "provider_api_key",
        "database_url",
        "database_dsn",
        "database_password",
        "object_store_access_key",
        "object_store_secret_key",
        "object_store_endpoint",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not expose authentication, credential, or internal-location fields in a public release"
        );
    }
}

#[test]
fn restricted_prefix_cannot_hide_behind_research_participant_namespace() {
    for column_name in [
        "linkage_ref_research_participant_ref",
        "keyverse_subject_research_participant_ref",
        "participant_ref_research_participant_ref",
        "participant_research_participant_ref",
        "subject_research_participant_ref",
        "linkage_research_participant_ref",
        "keyverse_research_participant_ref",
        "pseudonym_research_participant_ref",
        "operational_research_participant_ref",
        "assessment_research_participant_ref",
        "identity_research_participant_ref",
        "linked_research_participant_ref",
        "identity-subject-ref-research-participant-ref",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not bypass restricted-identity review by appending the public research namespace"
        );
    }
}

#[test]
fn prefixed_research_participant_namespace_remains_public() {
    for column_name in [
        "public_research_participant_ref",
        "exportResearchParticipantRef",
        "export-research-participant-ref",
        "research.participant.ref",
        "warehouse/research/participant/ref",
        "public research participant ref",
    ] {
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
fn structured_cells_must_be_flattened_before_identity_scanning() {
    for cell_value in [
        r#"{\"participant_ref\":\"participant_seoul_clinic_one\"}"#,
        r#"[{\"subject_ref\":\"keyverse_subject_seoul_clinic_one\"}]"#,
        "  {\"linkage_ref\":\"linkage_seoul_clinic_one\"}  ",
        "\n[\"linkage_key_version_2026_q3\"]\t",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name: "analysis_payload",
            cell_values: &[cell_value],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::StructuredValueUnsupported),
            "structured fixture cells must fail closed instead of bypassing exact identity matching"
        );
    }

    let message = PublicReleaseLeakageError::StructuredValueUnsupported.to_string();
    assert_eq!(
        message,
        "flatten or independently scan structured public-release values before packaging the fixture"
    );
    assert!(!message.contains("participant_seoul_clinic_one"));
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
