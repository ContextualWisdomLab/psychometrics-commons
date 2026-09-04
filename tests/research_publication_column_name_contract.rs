//! Confirms public research-release column-name boundaries without overblocking ordinary data labels.

use psychometrics_commons_runtime::research_release::{
    scan_public_release_fixture, PublicReleaseFixtureColumn, PublicReleaseLeakageError,
    RestrictedReleaseIdentities,
};

const OPERATIONAL_IDENTITIES: &[&str] = &["participant_alpha"];
const NO_IDENTITIES: &[&str] = &[];

fn restricted_identities() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: OPERATIONAL_IDENTITIES,
        keyverse_subject_refs: NO_IDENTITIES,
        linkage_refs: NO_IDENTITIES,
        linkage_key_versions: NO_IDENTITIES,
    }
}

#[test]
fn blank_public_release_column_names_fail_closed() {
    for column_name in ["", " ", "\t"] {
        let cell_values = ["public_value_alpha"];
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &cell_values,
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "blank public-release column names must not count as validated schema evidence"
        );
    }
}

#[test]
fn ordinary_participant_descriptive_columns_remain_publishable() {
    for column_name in ["participant_score", "participant_group", "participant_wave"] {
        let cell_values = ["public_value_alpha"];
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &cell_values,
        }];

        assert_eq!(
            scan_public_release_fixture(&columns, restricted_identities()),
            Ok(()),
            "{column_name} describes public research data without exposing a participant identity reference"
        );
    }
}
