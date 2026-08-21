//! Regression coverage for generic credential-shaped public-release columns.

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
fn compound_credential_words_fail_closed() {
    for column_name in [
        "session_token",
        "sessionToken",
        "SESSIONTOKEN",
        "user_password",
        "userPassword",
        "api_secret",
        "apiSecret",
        "service_credential",
        "serviceCredential",
        "auth_header",
        "authentication_header",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not expose a compound authentication or credential field"
        );
    }
}

#[test]
fn benign_author_and_public_research_columns_remain_allowed() {
    for column_name in [
        "author",
        "author_name",
        "authorResearchNote",
        "research_participant_ref",
        "export_research_participant_ref",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Ok(()),
            "{column_name} must remain a valid public-release column"
        );
    }
}
