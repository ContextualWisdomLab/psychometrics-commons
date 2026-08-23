//! Exact-spelling contracts for product-side Research Commons release evidence.

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
fn every_release_evidence_identity_rejects_padded_aliases() {
    for field in 0_usize..12 {
        let mut value = candidate();
        match field {
            0 => value.release_ref = " research_release_alpha",
            1 => value.dataset_snapshot_ref = "dataset_snapshot_alpha\u{00a0}",
            2 => value.research_scope_ref = "\u{2003}research_scope_alpha",
            3 => value.privacy_review_ref = "privacy_review_alpha\u{202f}",
            4 => value.scientific_review_ref = "\u{3000}scientific_review_alpha",
            5 => value.metadata_bundle_ref = "metadata_bundle_alpha ",
            6 => value.license_record_ref = " license_record_alpha",
            7 => value.measurement_provenance_ref = "measurement_provenance_alpha\u{00a0}",
            8 => value.access_approval_ref = "\u{2003}access_approval_alpha",
            9 => value.citation_metadata_ref = "citation_metadata_alpha\u{202f}",
            10 => value.release_approver_ref = "\u{3000}research_release_approver_alpha",
            11 => value.ordinary_admin_ref = "ordinary_admin_alpha ",
            _ => unreachable!(),
        }

        assert_eq!(
            approve_research_release(value),
            Err(ResearchReleaseGateError::InvalidReference),
            "field {field} must preserve exact caller spelling instead of trimming an alias"
        );
    }
}

#[test]
fn valid_multilingual_release_references_are_preserved_exactly() {
    let mut value = candidate();
    value.release_ref = "연구_release_α";
    value.dataset_snapshot_ref = "데이터_snapshot_β";
    value.release_approver_ref = "검토자_γ";
    value.ordinary_admin_ref = "관리자_δ";

    let approved = approve_research_release(value).unwrap();
    assert_eq!(approved.release_ref(), "연구_release_α");
    assert_eq!(approved.dataset_snapshot_ref(), "데이터_snapshot_β");
    assert_eq!(approved.release_approver_ref(), "검토자_γ");
    assert_eq!(approved.ordinary_admin_ref(), "관리자_δ");
}
