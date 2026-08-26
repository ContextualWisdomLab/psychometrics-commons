//! Executable gate coverage for product-side Research Commons release approval.

use psychometrics_commons_runtime::research_release::{
    approve_research_release, ResearchAccessClass, ResearchReleaseCandidate,
    ResearchReleaseGateError,
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
    value.release_approver_ref = "research_release_approver_alpha";
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
            "research release references must use the exact opaque non-numeric spelling",
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
