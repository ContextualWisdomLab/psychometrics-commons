//! Domain contract for restricted operational-to-research identity linkage.
//!
//! A buyer contributing assessment data to Research Commons must keep the
//! operational participant out of any public release fixture. This contract
//! proves the linkage exists for authorized research work, that one person can
//! hold distinct program-scoped research identities, and that a public
//! projection cannot carry the operational reference or linkage-key version.

use psychometrics_commons_runtime::research_identity_linkage::{
    RestrictedIdentityLinkage, RestrictedIdentityLinkageError,
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
