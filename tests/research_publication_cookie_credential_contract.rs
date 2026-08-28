//! Regression coverage for HTTP cookie credentials in public release columns.

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
fn session_and_http_cookie_credential_columns_fail_closed() {
    for column_name in [
        "session_cookie",
        "browser_session_cookie",
        "cookie_header",
        "set_cookie",
        "Set-Cookie",
        "sessionCookie",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["opaque_cookie_material"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} can contain authentication credentials and must not enter a public release"
        );
    }
}

#[test]
fn noncredential_cookie_governance_metadata_remains_publishable() {
    for column_name in ["cookie_consent_status", "cookie_policy_version"] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["public_metadata"],
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Ok(()),
            "{column_name} is policy metadata rather than cookie credential material"
        );
    }
}
