//! Contract tests for ADR-0019 scientific publication evidence gating.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    PublicationState,
};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
const EVIDENCE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn manifest() -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        "release_big_five_ko_v1",
        "instrument_big_five",
        "instrument_version_big_five_ko_v1",
        "construct_big_five",
        &["item_version_001", "item_version_002"],
        "ko-KR",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "narrative_version_big_five_v1",
        &["consent_service_v1"],
        "intended_use_self_reflection_v1",
        "limitations_nonclinical_v1",
        VALID_DIGEST,
    )
    .unwrap()
}

fn provenance() -> PublicationEvidenceProvenance {
    PublicationEvidenceProvenance::new(
        EVIDENCE_DIGEST,
        "population_general_adult_v1",
        "administration_web_self_report_v1",
        "measurement_model_big_five_v1",
        10_050,
        None,
    )
    .unwrap()
}

fn evidence(
    status: PublicationEvidenceStatus,
    digest: &str,
    content_rights_evidence_refs: &[&str],
    scientific_evidence_refs: &[&str],
    approval_refs: &[&str],
) -> Result<PublicationEvidenceRecord, InstrumentReleaseError> {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        digest,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        provenance(),
        content_rights_evidence_refs,
        scientific_evidence_refs,
        approval_refs,
        status,
    )
}

fn reviewed_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
}

#[test]
fn review_to_publish_fails_closed_without_approved_evidence() {
    let mut missing = reviewed_release();
    assert_eq!(
        missing.apply_command("publish_event", PublicationCommand::Publish, 10_200),
        Err(InstrumentReleaseError::MissingPublicationEvidence)
    );
    assert_eq!(missing.state(), PublicationState::Review);

    for status in [
        PublicationEvidenceStatus::Failed,
        PublicationEvidenceStatus::Unknown,
    ] {
        let mut release = reviewed_release();
        let record = evidence(status, VALID_DIGEST, &[], &[], &[]).unwrap();
        release.bind_publication_evidence(record).unwrap();
        assert_eq!(
            release.apply_command("publish_event", PublicationCommand::Publish, 10_200),
            Err(InstrumentReleaseError::PublicationEvidenceNotApproved),
            "{status:?} evidence must not publish the release"
        );
        assert_eq!(release.state(), PublicationState::Review);
    }
}

#[test]
fn approved_exact_evidence_is_auditable_and_allows_publication() {
    let mut release = reviewed_release();
    let approved = evidence(
        PublicationEvidenceStatus::Approved,
        VALID_DIGEST,
        &["rights_ipip_big_five_v1", "content_review_big_five_ko_v1"],
        &[
            "recovery_big_five_ko_v1",
            "scoreability_big_five_ko_v1",
            "dif_review_big_five_ko_v1",
        ],
        &["approval_psychometrics_big_five_ko_v1"],
    )
    .unwrap();

    release.bind_publication_evidence(approved).unwrap();
    let bound = release.publication_evidence().unwrap();
    assert_eq!(
        bound.publication_evidence_ref(),
        "publication_evidence_big_five_ko_v1"
    );
    assert_eq!(
        bound.evidence_policy_ref(),
        "evidence_policy_self_reflection_v1"
    );
    assert_eq!(bound.status(), PublicationEvidenceStatus::Approved);
    assert_eq!(
        bound.content_rights_evidence_refs(),
        ["rights_ipip_big_five_v1", "content_review_big_five_ko_v1"]
    );
    assert_eq!(
        bound.scientific_evidence_refs(),
        [
            "recovery_big_five_ko_v1",
            "scoreability_big_five_ko_v1",
            "dif_review_big_five_ko_v1"
        ]
    );
    assert_eq!(
        bound.approval_refs(),
        ["approval_psychometrics_big_five_ko_v1"]
    );

    assert_eq!(
        release
            .apply_command("publish_event", PublicationCommand::Publish, 10_200)
            .unwrap(),
        PublicationState::Published
    );
    assert!(release.accepts_new_sessions());
}

#[test]
fn publication_evidence_must_bind_the_exact_release_bundle() {
    let mut release = reviewed_release();
    let wrong_digest = evidence(
        PublicationEvidenceStatus::Approved,
        OTHER_DIGEST,
        &["rights_ipip_big_five_v1"],
        &["recovery_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
    )
    .unwrap();

    assert_eq!(
        release.bind_publication_evidence(wrong_digest),
        Err(InstrumentReleaseError::PublicationEvidenceMismatch)
    );
    assert!(release.publication_evidence().is_none());
}

#[test]
fn approved_evidence_cannot_omit_content_rights_science_or_approval_evidence() {
    for (content_rights, scientific, approvals) in [
        (
            &[][..],
            &["recovery_big_five_ko_v1"][..],
            &["approval_psychometrics_big_five_ko_v1"][..],
        ),
        (
            &["rights_ipip_big_five_v1"][..],
            &[][..],
            &["approval_psychometrics_big_five_ko_v1"][..],
        ),
        (
            &["rights_ipip_big_five_v1"][..],
            &["recovery_big_five_ko_v1"][..],
            &[][..],
        ),
    ] {
        assert_eq!(
            evidence(
                PublicationEvidenceStatus::Approved,
                VALID_DIGEST,
                content_rights,
                scientific,
                approvals,
            ),
            Err(InstrumentReleaseError::IncompletePublicationEvidence)
        );
    }
}
