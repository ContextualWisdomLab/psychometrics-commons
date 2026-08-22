//! Executable gate coverage for product-side Research Commons release approval.

use psychometrics_commons_runtime::research_release::{
    approve_research_release, scan_public_release_fixture, PublicReleaseFixtureColumn,
    PublicReleaseLeakageError, ResearchAccessClass, ResearchReleaseCandidate,
    ResearchReleaseGateError, RestrictedReleaseIdentities,
};

const MANIFEST_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn candidate() -> ResearchReleaseCandidate<'static> {
    ResearchReleaseCandidate {
        release_ref: "research_release_alpha",
        dataset_snapshot_ref: "dataset_snapshot_alpha",
        research_scope_ref: "research_scope_alpha",
        manifest_digest: MANIFEST_DIGEST,
        privacy_review_ref: "privacy_review_alpha",
        scientific_review_ref: "scientific_review_alpha",
        metadata_bundle_ref: "metadata_bundle_alpha",
        license_record_ref: "license_record_alpha",
        measurement_provenance_ref: "measurement_provenance_alpha",
        access_approval_ref: "access_approval_alpha",
        citation_metadata_ref: "citation_metadata_alpha",
        release_approver_ref: "research_release_approver_alpha",
        ordinary_admin_ref: "ordinary_admin_alpha",
        unresolved_blocking_findings: 0,
        access_class: ResearchAccessClass::Controlled,
    }
}

#[test]
fn complete_release_evidence_is_approved_without_claiming_portal_publication() {
    let approved = approve_research_release(candidate()).unwrap();

    assert_eq!(approved.release_ref(), "research_release_alpha");
    assert_eq!(approved.dataset_snapshot_ref(), "dataset_snapshot_alpha");
    assert_eq!(approved.research_scope_ref(), "research_scope_alpha");
    assert_eq!(approved.manifest_digest(), MANIFEST_DIGEST);
    assert_eq!(approved.privacy_review_ref(), "privacy_review_alpha");
    assert_eq!(approved.scientific_review_ref(), "scientific_review_alpha");
    assert_eq!(approved.metadata_bundle_ref(), "metadata_bundle_alpha");
    assert_eq!(approved.license_record_ref(), "license_record_alpha");
    assert_eq!(
        approved.measurement_provenance_ref(),
        "measurement_provenance_alpha"
    );
    assert_eq!(approved.access_approval_ref(), "access_approval_alpha");
    assert_eq!(approved.citation_metadata_ref(), "citation_metadata_alpha");
    assert_eq!(
        approved.release_approver_ref(),
        "research_release_approver_alpha"
    );
    assert_eq!(approved.ordinary_admin_ref(), "ordinary_admin_alpha");
    assert_eq!(approved.access_class(), ResearchAccessClass::Controlled);
}

#[test]
fn every_required_reference_fails_closed_when_missing_or_nonopaque() {
    for field in 0_usize..12 {
        let mut value = candidate();
        match field {
            0 => value.release_ref = "12345",
            1 => value.dataset_snapshot_ref = " ",
            2 => value.research_scope_ref = "12345",
            3 => value.privacy_review_ref = " ",
            4 => value.scientific_review_ref = "12345",
            5 => value.metadata_bundle_ref = " ",
            6 => value.license_record_ref = "12345",
            7 => value.measurement_provenance_ref = " ",
            8 => value.access_approval_ref = "12345",
            9 => value.citation_metadata_ref = " ",
            10 => value.release_approver_ref = "12345",
            11 => value.ordinary_admin_ref = " ",
            _ => unreachable!(),
        }
        assert_eq!(
            approve_research_release(value),
            Err(ResearchReleaseGateError::InvalidReference)
        );
    }
}

#[test]
fn manifest_digest_must_be_exact_lowercase_sha256_evidence() {
    for invalid in [
        "manifest-alpha",
        "sha512:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "sha256:0123456789abcdef",
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
        "sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        let mut value = candidate();
        value.manifest_digest = invalid;
        assert_eq!(
            approve_research_release(value),
            Err(ResearchReleaseGateError::InvalidManifestDigest)
        );
    }
}

#[test]
fn unresolved_release_blockers_prevent_approval() {
    let mut value = candidate();
    value.unresolved_blocking_findings = 1;

    assert_eq!(
        approve_research_release(value),
        Err(ResearchReleaseGateError::UnresolvedBlockingFinding)
    );
}

#[test]
fn release_approval_is_separated_from_ordinary_administration() {
    let mut value = candidate();
    value.release_approver_ref = " research_release_approver_alpha ";
    value.ordinary_admin_ref = "research_release_approver_alpha";

    assert_eq!(
        approve_research_release(value),
        Err(ResearchReleaseGateError::SeparationOfDutiesViolation)
    );
}

#[test]
fn all_access_classes_remain_explicit_release_evidence() {
    for access_class in [
        ResearchAccessClass::Public,
        ResearchAccessClass::Controlled,
        ResearchAccessClass::Private,
        ResearchAccessClass::Embargoed,
    ] {
        let mut value = candidate();
        value.access_class = access_class;
        assert_eq!(
            approve_research_release(value).unwrap().access_class(),
            access_class
        );
    }
}

#[test]
fn release_gate_errors_have_stable_safe_operator_messages() {
    let cases = [
        (
            ResearchReleaseGateError::InvalidReference,
            "research release references must be opaque non-numeric values",
        ),
        (
            ResearchReleaseGateError::InvalidManifestDigest,
            "research release manifest digest must be canonical sha256 evidence",
        ),
        (
            ResearchReleaseGateError::UnresolvedBlockingFinding,
            "research release has unresolved blocking findings",
        ),
        (
            ResearchReleaseGateError::SeparationOfDutiesViolation,
            "research release approver must be independent from ordinary administration",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

fn seoul_clinic_restricted_identities() -> RestrictedReleaseIdentities<'static> {
    RestrictedReleaseIdentities {
        operational_participant_refs: &["participant_seoul_clinic_one"],
        keyverse_subject_refs: &["keyverse_subject_seoul_clinic_one"],
        linkage_refs: &["linkage_seoul_clinic_one"],
        linkage_key_versions: &["linkage_key_version_2026_q3"],
    }
}

fn public_research_identity_columns() -> [PublicReleaseFixtureColumn<'static>; 2] {
    [
        PublicReleaseFixtureColumn {
            column_name: "research_participant_ref",
            cell_values: &["research_participant_program_alpha_one"],
        },
        PublicReleaseFixtureColumn {
            column_name: "research_program_ref",
            cell_values: &["research_program_alpha"],
        },
    ]
}

#[test]
fn seoul_clinic_public_fixture_keeps_only_research_identities() {
    assert_eq!(
        scan_public_release_fixture(
            &public_research_identity_columns(),
            seoul_clinic_restricted_identities()
        ),
        Ok(())
    );
}

#[test]
fn operational_participant_column_fails_closed_even_when_cells_look_like_research_ids() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: " participant_ref ",
        cell_values: &["research_participant_program_alpha_one"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
        Err(PublicReleaseLeakageError::ForbiddenColumn)
    );
}

#[test]
fn research_participant_column_is_not_treated_as_an_operational_participant_column() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "research_participant_ref",
        cell_values: &["research_participant_program_alpha_one"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
        Ok(())
    );
}

#[test]
fn keyverse_and_linkage_columns_cannot_enter_a_public_fixture() {
    for column_name in [
        "keyverse_subject_ref",
        "linkage_ref",
        "linkage_key_version",
        "operational_participant_ref",
        "participant_id",
        "keyverse_subject",
        "linkage_key",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];
        assert_eq!(
            scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} must not appear in a public release fixture"
        );
    }
}

#[test]
fn governance_and_product_identity_columns_cannot_enter_a_public_fixture() {
    for column_name in [
        "assessment_participant_ref",
        "pseudonym_key_version",
        "identity_subject_ref",
        "subject_ref",
        "linked_subject_ref",
        "Assessment_Participant_Ref",
        "assessmentParticipantRef",
        "SUBJECT_REF",
        "linkedSubjectRef",
        "pseudonymKeyVersion",
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name,
            cell_values: &["research_participant_program_alpha_one"],
        }];
        assert_eq!(
            scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
            Err(PublicReleaseLeakageError::ForbiddenColumn),
            "{column_name} is a governance or product identity column and must not appear in a public release fixture"
        );
    }
}

#[test]
fn camel_case_research_participant_column_stays_allowed() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "researchParticipantRef",
        cell_values: &["research_participant_program_alpha_one"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
        Ok(())
    );
}

#[test]
fn operational_participant_value_fails_closed_inside_an_otherwise_public_column() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "research_participant_ref",
        cell_values: &[" participant_seoul_clinic_one "],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
        Err(PublicReleaseLeakageError::OperationalParticipant)
    );
}

#[test]
fn keyverse_subject_value_fails_closed_inside_an_otherwise_public_column() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "theta_estimate",
        cell_values: &["keyverse_subject_seoul_clinic_one"],
    }];

    assert_eq!(
        scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
        Err(PublicReleaseLeakageError::KeyverseSubject)
    );
}

#[test]
fn restricted_linkage_values_fail_closed_inside_an_otherwise_public_column() {
    for (value, expected) in [
        (
            "linkage_seoul_clinic_one",
            PublicReleaseLeakageError::RestrictedLinkage,
        ),
        (
            "linkage_key_version_2026_q3",
            PublicReleaseLeakageError::RestrictedLinkage,
        ),
    ] {
        let columns = [PublicReleaseFixtureColumn {
            column_name: "research_program_ref",
            cell_values: &[value],
        }];
        assert_eq!(
            scan_public_release_fixture(&columns, seoul_clinic_restricted_identities()),
            Err(expected)
        );
    }
}

#[test]
fn public_release_leakage_errors_tell_the_operator_what_to_remove() {
    let cases = [
        (
            PublicReleaseLeakageError::ForbiddenColumn,
            "remove restricted identity, authentication, credential, or internal-location columns from the public release fixture",
        ),
        (
            PublicReleaseLeakageError::OperationalParticipant,
            "remove operational participant identifiers from the public release fixture",
        ),
        (
            PublicReleaseLeakageError::KeyverseSubject,
            "remove Keyverse subject identifiers from the public release fixture",
        ),
        (
            PublicReleaseLeakageError::RestrictedLinkage,
            "remove restricted linkage identifiers and linkage-key versions from the public release fixture",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn missing_restricted_identity_inventory_fails_closed_before_public_packaging() {
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &[],
        keyverse_subject_refs: &["  "],
        linkage_refs: &[],
        linkage_key_versions: &[""],
    };

    assert_eq!(
        scan_public_release_fixture(&public_research_identity_columns(), restricted),
        Err(PublicReleaseLeakageError::IdentityInventoryUnavailable)
    );
}

#[test]
fn any_effective_restricted_identity_inventory_can_prove_the_scan_was_supplied() {
    let cases = [
        RestrictedReleaseIdentities {
            operational_participant_refs: &["participant_inventory_evidence"],
            keyverse_subject_refs: &[],
            linkage_refs: &[],
            linkage_key_versions: &[],
        },
        RestrictedReleaseIdentities {
            operational_participant_refs: &[],
            keyverse_subject_refs: &["keyverse_inventory_evidence"],
            linkage_refs: &[],
            linkage_key_versions: &[],
        },
        RestrictedReleaseIdentities {
            operational_participant_refs: &[],
            keyverse_subject_refs: &[],
            linkage_refs: &["linkage_inventory_evidence"],
            linkage_key_versions: &[],
        },
        RestrictedReleaseIdentities {
            operational_participant_refs: &[],
            keyverse_subject_refs: &[],
            linkage_refs: &[],
            linkage_key_versions: &["pseudonym_key_inventory_evidence"],
        },
    ];

    for restricted in cases {
        assert_eq!(
            scan_public_release_fixture(&public_research_identity_columns(), restricted),
            Ok(())
        );
    }
}

#[test]
fn forbidden_column_is_reported_even_when_identity_inventory_is_unavailable() {
    let columns = [PublicReleaseFixtureColumn {
        column_name: "assessmentParticipantRef",
        cell_values: &["research_participant_program_alpha_one"],
    }];
    let restricted = RestrictedReleaseIdentities {
        operational_participant_refs: &[],
        keyverse_subject_refs: &[],
        linkage_refs: &[],
        linkage_key_versions: &[],
    };

    assert_eq!(
        scan_public_release_fixture(&columns, restricted),
        Err(PublicReleaseLeakageError::ForbiddenColumn)
    );
}
