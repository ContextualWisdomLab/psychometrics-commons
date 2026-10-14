//! Regression coverage for session and JWT credential columns in public releases.

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
fn generic_session_identifiers_and_jwt_credentials_fail_closed() {
    for column_name in [
        "session_id",
        "web_session_id",
        "browserSessionId",
        "jwt",
        "session_jwt",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_authentication_material"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} can link to an operational session or carry bearer credentials and must not enter a public release"
        );
    }
}

#[test]
fn research_session_index_metadata_remains_publishable() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "research_session_index",
        cell_values: &["1"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, restricted_identities()),
        Ok(())
    );
}
