//! Regression coverage for renewing publication evidence before reactivation.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    PublicationState,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_RELEASE_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const INITIAL_EVIDENCE_DIGEST: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
const RENEWED_EVIDENCE_DIGEST: &str =
    "sha256:123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0";

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
        RELEASE_DIGEST,
    )
    .unwrap()
}

fn evidence(
    evidence_ref: &str,
    evidence_digest: &str,
    evaluated_at_unix_ms: u64,
    valid_until_unix_ms: u64,
) -> PublicationEvidenceRecord {
    evidence_for_content_digest(
        evidence_ref,
        evidence_digest,
        evaluated_at_unix_ms,
        valid_until_unix_ms,
        RELEASE_DIGEST,
    )
}

fn evidence_for_content_digest(
    evidence_ref: &str,
    evidence_digest: &str,
    evaluated_at_unix_ms: u64,
    valid_until_unix_ms: u64,
    content_digest: &str,
) -> PublicationEvidenceRecord {
    PublicationEvidenceRecord::new(
        evidence_ref,
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        &["item_version_001", "item_version_002"],
        content_digest,
        "ko-KR",
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        PublicationEvidenceProvenance::new(
            evidence_digest,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            evaluated_at_unix_ms,
            Some(valid_until_unix_ms),
        )
        .unwrap(),
        &["rights_ipip_big_five_v1", "content_review_big_five_ko_v1"],
        &["recovery_big_five_ko_v1", "scoreability_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(manifest(), 10_000).unwrap();
    release
        .apply_command(
            "submit_review_event",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(evidence(
            "publication_evidence_big_five_ko_initial",
            INITIAL_EVIDENCE_DIGEST,
            10_150,
            10_250,
        ))
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 10_200)
        .unwrap();
    release
}

#[test]
fn suspended_release_can_bind_renewed_evidence_before_reactivation() {
    let mut release = published_release();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 10_220)
        .unwrap();

    assert_eq!(
        release.apply_command(
            "expired_reactivate_event",
            PublicationCommand::Reactivate,
            10_251,
        ),
        Err(InstrumentReleaseError::PublicationEvidenceNotEffective)
    );
    assert_eq!(release.state(), PublicationState::Suspended);

    release
        .bind_publication_evidence(evidence(
            "publication_evidence_big_five_ko_renewed",
            RENEWED_EVIDENCE_DIGEST,
            10_240,
            10_350,
        ))
        .unwrap();

    assert_eq!(
        release
            .apply_command(
                "renewed_reactivate_event",
                PublicationCommand::Reactivate,
                10_260,
            )
            .unwrap(),
        PublicationState::Published
    );

    let initial_publish = &release.events()[1];
    assert_eq!(
        initial_publish.publication_evidence_ref(),
        Some("publication_evidence_big_five_ko_initial")
    );
    assert_eq!(
        initial_publish.publication_evidence_digest(),
        Some(INITIAL_EVIDENCE_DIGEST)
    );

    let reactivation = release.events().last().unwrap();
    assert_eq!(reactivation.command(), PublicationCommand::Reactivate);
    assert_eq!(
        reactivation.publication_evidence_ref(),
        Some("publication_evidence_big_five_ko_renewed")
    );
    assert_eq!(
        reactivation.publication_evidence_digest(),
        Some(RENEWED_EVIDENCE_DIGEST)
    );
}

#[test]
fn mismatched_renewal_preserves_previously_bound_evidence() {
    let mut release = published_release();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 10_220)
        .unwrap();
    let initially_bound_ref = release
        .publication_evidence()
        .unwrap()
        .publication_evidence_ref()
        .to_owned();
    let initially_bound_digest = release
        .publication_evidence()
        .unwrap()
        .provenance()
        .evidence_digest()
        .to_owned();

    assert_eq!(
        release.bind_publication_evidence(evidence_for_content_digest(
            "publication_evidence_big_five_ko_mismatched",
            RENEWED_EVIDENCE_DIGEST,
            10_240,
            10_350,
            OTHER_RELEASE_DIGEST,
        )),
        Err(InstrumentReleaseError::PublicationEvidenceMismatch)
    );
    assert_eq!(release.state(), PublicationState::Suspended);
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .publication_evidence_ref(),
        initially_bound_ref
    );
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .provenance()
            .evidence_digest(),
        initially_bound_digest
    );
}

#[test]
fn published_release_cannot_replace_its_bound_publication_evidence() {
    let mut release = published_release();
    let initially_bound_ref = release
        .publication_evidence()
        .unwrap()
        .publication_evidence_ref()
        .to_owned();
    let initially_bound_digest = release
        .publication_evidence()
        .unwrap()
        .provenance()
        .evidence_digest()
        .to_owned();

    assert_eq!(
        release.bind_publication_evidence(evidence(
            "publication_evidence_big_five_ko_renewed",
            RENEWED_EVIDENCE_DIGEST,
            10_210,
            10_350,
        )),
        Err(InstrumentReleaseError::InvalidTransition)
    );
    assert_eq!(release.state(), PublicationState::Published);
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .publication_evidence_ref(),
        initially_bound_ref
    );
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .provenance()
            .evidence_digest(),
        initially_bound_digest
    );
}

#[test]
fn retired_release_cannot_bind_renewed_publication_evidence() {
    let mut release = published_release();
    release
        .apply_command("retire_event", PublicationCommand::Retire, 10_220)
        .unwrap();
    let initially_bound_ref = release
        .publication_evidence()
        .unwrap()
        .publication_evidence_ref()
        .to_owned();
    let initially_bound_digest = release
        .publication_evidence()
        .unwrap()
        .provenance()
        .evidence_digest()
        .to_owned();

    assert_eq!(
        release.bind_publication_evidence(evidence(
            "publication_evidence_big_five_ko_renewed",
            RENEWED_EVIDENCE_DIGEST,
            10_210,
            10_350,
        )),
        Err(InstrumentReleaseError::InvalidTransition)
    );
    assert_eq!(release.state(), PublicationState::Retired);
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .publication_evidence_ref(),
        initially_bound_ref
    );
    assert_eq!(
        release
            .publication_evidence()
            .unwrap()
            .provenance()
            .evidence_digest(),
        initially_bound_digest
    );
}
