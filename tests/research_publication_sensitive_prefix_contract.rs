//! Fail-closed coverage for sensitive prefixes masking the public research namespace.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

fn restricted_identities() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_inventory_evidence"],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    }
}

#[test]
fn sensitive_prefixes_cannot_hide_behind_research_participant_namespace() {
    for column_name in [
        "auth_research_participant_ref",
        "secret_research_participant_ref",
        "secrets_research_participant_ref",
        "token_research_participant_ref",
        "tokens_research_participant_ref",
        "password_research_participant_ref",
        "passwords_research_participant_ref",
        "credential_research_participant_ref",
        "credentials_research_participant_ref",
        "database_research_participant_ref",
        "object_store_research_participant_ref",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_public_value"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not use the public research namespace to mask a sensitive field"
        );
    }
}

#[test]
fn bare_sensitive_marker_columns_fail_closed() {
    for column_name in [
        "assessment",
        "auth",
        "credential",
        "database",
        "identity",
        "keyverse",
        "linkage",
        "linked",
        "object_store",
        "operational",
        "participant",
        "password",
        "pseudonym",
        "secret",
        "subject",
        "token",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_public_value"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} is itself a sensitive namespace marker and must fail closed"
        );
    }
}
