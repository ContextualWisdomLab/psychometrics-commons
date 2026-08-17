//! Domain contract for restricted operational-to-research identity linkage.
//!
//! A buyer contributing assessment data to Research Commons must keep the
//! operational participant out of any public release fixture. This contract
//! proves the linkage exists for authorized research work, that one person can
//! hold distinct program-scoped research identities, and that a public
//! projection cannot carry the operational reference or linkage-key version.

use psychometrics_commons_runtime::research_identity_linkage::{
    PublicResearchReleaseProjection, RestrictedIdentityLinkage, RestrictedIdentityLinkageError,
};

fn valid_linkage() -> RestrictedIdentityLinkage {
    RestrictedIdentityLinkage::new(
        "linkage_commons_program_one",
        "participant_operational_one",
        "research_participant_program_one",
        "research_program_commons_one",
        "linkage_key_version_2026_q3",
        1_724_000_000_000,
    )
    .expect("valid restricted linkage must construct")
}

#[test]
fn linkage_rejects_blank_padded_and_numeric_references() {
    for (
        linkage_ref,
        participant_ref,
        research_participant_ref,
        research_program_ref,
        key_version,
    ) in [
        (
            "",
            "participant_operational_one",
            "research_participant_one",
            "research_program_one",
            "linkage_key_version_one",
        ),
        (
            " linkage_one",
            "participant_operational_one",
            "research_participant_one",
            "research_program_one",
            "linkage_key_version_one",
        ),
        (
            "12",
            "participant_operational_one",
            "research_participant_one",
            "research_program_one",
            "linkage_key_version_one",
        ),
        (
            "linkage_one",
            "",
            "research_participant_one",
            "research_program_one",
            "linkage_key_version_one",
        ),
        (
            "linkage_one",
            "participant_operational_one",
            "1e3",
            "research_program_one",
            "linkage_key_version_one",
        ),
        (
            "linkage_one",
            "participant_operational_one",
            "research_participant_one",
            " ",
            "linkage_key_version_one",
        ),
        (
            "linkage_one",
            "participant_operational_one",
            "research_participant_one",
            "research_program_one",
            "",
        ),
    ] {
        let error = RestrictedIdentityLinkage::new(
            linkage_ref,
            participant_ref,
            research_participant_ref,
            research_program_ref,
            key_version,
            1_724_000_000_000,
        )
        .expect_err("unknown or padded linkage identity must fail closed");
        assert_eq!(error, RestrictedIdentityLinkageError::InvalidReference);
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn linkage_rejects_reusing_the_operational_participant_as_research_identity() {
    let error = RestrictedIdentityLinkage::new(
        "linkage_commons_program_one",
        "participant_operational_one",
        "participant_operational_one",
        "research_program_commons_one",
        "linkage_key_version_2026_q3",
        1_724_000_000_000,
    )
    .expect_err("research identity must not be the operational participant");
    assert_eq!(
        error,
        RestrictedIdentityLinkageError::OperationalIdentityReuse
    );
    assert!(error.to_string().contains("operational"));
}

#[test]
fn linkage_rejects_zero_recorded_time() {
    let error = RestrictedIdentityLinkage::new(
        "linkage_commons_program_one",
        "participant_operational_one",
        "research_participant_program_one",
        "research_program_commons_one",
        "linkage_key_version_2026_q3",
        0,
    )
    .expect_err("linkage recorded time must be a real platform instant");
    assert_eq!(error, RestrictedIdentityLinkageError::InvalidRecordedTime);
}

#[test]
fn one_operational_participant_keeps_distinct_program_scoped_research_identities() {
    let first = valid_linkage();
    let second = RestrictedIdentityLinkage::new(
        "linkage_commons_program_two",
        first.participant_ref(),
        "research_participant_program_two",
        "research_program_commons_two",
        "linkage_key_version_2026_q3",
        1_724_000_100_000,
    )
    .expect("a second program must mint a distinct research identity");

    assert_eq!(first.participant_ref(), second.participant_ref());
    assert_ne!(
        first.research_participant_ref(),
        second.research_participant_ref()
    );
    assert_ne!(first.research_program_ref(), second.research_program_ref());
    assert_ne!(first.linkage_ref(), second.linkage_ref());
}

#[test]
fn public_release_projection_omits_operational_identity_and_linkage_key_version() {
    let linkage = valid_linkage();
    let projection = linkage.public_release_projection();

    assert_eq!(
        projection.research_participant_ref(),
        "research_participant_program_one"
    );
    assert_eq!(
        projection.research_program_ref(),
        "research_program_commons_one"
    );
    let rendered = format!("{projection:?}");
    assert!(!rendered.contains(linkage.participant_ref()));
    assert!(!rendered.contains(linkage.linkage_ref()));
    assert!(!rendered.contains(linkage.linkage_key_version()));
    assert!(!rendered.contains("participant_operational_one"));
    assert!(!rendered.contains("linkage_key_version_2026_q3"));
}

#[test]
fn public_projection_constructs_from_research_identities_only() {
    let projection = PublicResearchReleaseProjection::new(
        "research_participant_program_one",
        "research_program_commons_one",
    )
    .expect("a public projection must construct from research identities alone");
    assert_eq!(
        projection.research_participant_ref(),
        "research_participant_program_one"
    );
    assert_eq!(
        projection.research_program_ref(),
        "research_program_commons_one"
    );
    let rendered = format!("{projection:?}");
    assert!(!rendered.contains("participant_operational"));
    assert!(!rendered.contains("linkage_key_version"));
}

#[test]
fn public_projection_rejects_blank_padded_numeric_or_collapsed_identities() {
    for (research_participant_ref, research_program_ref, expected) in [
        (
            "",
            "research_program_commons_one",
            RestrictedIdentityLinkageError::InvalidReference,
        ),
        (
            " research_participant_program_one",
            "research_program_commons_one",
            RestrictedIdentityLinkageError::InvalidReference,
        ),
        (
            "12",
            "research_program_commons_one",
            RestrictedIdentityLinkageError::InvalidReference,
        ),
        (
            "research_participant_program_one",
            "research_participant_program_one",
            RestrictedIdentityLinkageError::OperationalIdentityReuse,
        ),
    ] {
        let error =
            PublicResearchReleaseProjection::new(research_participant_ref, research_program_ref)
                .expect_err("public projection must fail closed without a restricted linkage");
        assert_eq!(error, expected);
    }
}

#[test]
fn public_release_adapter_does_not_load_by_restricted_linkage_ref() {
    let source = include_str!("../src/postgres_research_identity_linkage.rs");
    assert!(
        !source.contains("pub fn load_public_research_release_projection"),
        "a public-release fixture must load public_research_identity by program, not by restricted linkage_ref"
    );
    let public_load = source
        .split("pub fn load_public_research_identities_for_program")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .expect("program-scoped public load must exist");
    assert!(
        public_load.contains("FROM public_research_identity"),
        "public load must select the public view"
    );
    assert!(
        !public_load.contains("research_identity_linkage"),
        "public load must not read the restricted linkage table"
    );
    assert!(
        !public_load.contains("linkage_key_version") && !public_load.contains("linkage_ref"),
        "public load must not project linkage-key or restricted-linkage columns"
    );
}
