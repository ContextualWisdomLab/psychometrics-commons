//! ADR-0019 provenance, validity-window, and fail-closed branch coverage.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseError, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
    PublicationState,
};

const RELEASE_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EVIDENCE_DIGEST: &str =
    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

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

fn provenance(
    evaluated_at_unix_ms: u64,
    valid_until_unix_ms: Option<u64>,
) -> Result<PublicationEvidenceProvenance, InstrumentReleaseError> {
    PublicationEvidenceProvenance::new(
        EVIDENCE_DIGEST,
        "population_general_adult_v1",
        "administration_web_self_report_v1",
        "measurement_model_big_five_v1",
        evaluated_at_unix_ms,
        valid_until_unix_ms,
    )
}

fn evidence(
    item_version_refs: &[&str],
    content_digest: &str,
    locale: &str,
    provenance: PublicationEvidenceProvenance,
) -> Result<PublicationEvidenceRecord, InstrumentReleaseError> {
    PublicationEvidenceRecord::new(
        "publication_evidence_big_five_ko_v1",
        "evidence_policy_self_reflection_v1",
        "release_big_five_ko_v1",
        "instrument_version_big_five_ko_v1",
        item_version_refs,
        content_digest,
        locale,
        "intended_use_self_reflection_v1",
        "assessment_spec_big_five_v1",
        "scoring_version_big_five_v1",
        "calibration_big_five_ko_v1",
        Some("norm_version_big_five_ko_v1"),
        "limitations_nonclinical_v1",
        provenance,
        &["rights_ipip_big_five_v1", "content_review_big_five_ko_v1"],
        &["recovery_big_five_ko_v1", "scoreability_big_five_ko_v1"],
        &["approval_psychometrics_big_five_ko_v1"],
        PublicationEvidenceStatus::Approved,
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
fn publish_event_binds_exact_evidence_identity_digest_and_policy() {
    let mut release = reviewed_release();
    let approved = evidence(
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
        "ko-KR",
        provenance(10_150, Some(10_250)).unwrap(),
    )
    .unwrap();

    let audit = approved.provenance();
    assert_eq!(audit.evidence_digest(), EVIDENCE_DIGEST);
    assert_eq!(
        audit.population_context_ref(),
        "population_general_adult_v1"
    );
    assert_eq!(
        audit.administration_mode_ref(),
        "administration_web_self_report_v1"
    );
    assert_eq!(
        audit.measurement_model_ref(),
        "measurement_model_big_five_v1"
    );
    assert_eq!(audit.evaluated_at_unix_ms(), 10_150);
    assert_eq!(audit.valid_until_unix_ms(), Some(10_250));

    release.bind_publication_evidence(approved).unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 10_200)
        .unwrap();

    let event = release.events().last().unwrap();
    assert_eq!(
        event.publication_evidence_ref(),
        Some("publication_evidence_big_five_ko_v1")
    );
    assert_eq!(
        event.evidence_policy_ref(),
        Some("evidence_policy_self_reflection_v1")
    );
    assert_eq!(event.publication_evidence_digest(), Some(EVIDENCE_DIGEST));
}

#[test]
fn evidence_must_be_effective_at_the_server_authoritative_publish_time() {
    for provenance in [
        provenance(10_201, None).unwrap(),
        provenance(10_150, Some(10_199)).unwrap(),
    ] {
        let mut release = reviewed_release();
        release
            .bind_publication_evidence(
                evidence(
                    &["item_version_001", "item_version_002"],
                    RELEASE_DIGEST,
                    "ko-KR",
                    provenance,
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(
            release.apply_command("publish_event", PublicationCommand::Publish, 10_200),
            Err(InstrumentReleaseError::PublicationEvidenceNotEffective)
        );
        assert_eq!(release.state(), PublicationState::Review);
        assert_eq!(release.events().len(), 1);
    }
}

#[test]
fn evidence_is_effective_on_inclusive_window_boundaries() {
    for provenance in [
        provenance(10_200, None).unwrap(),
        provenance(10_150, Some(10_200)).unwrap(),
    ] {
        let mut release = reviewed_release();
        release
            .bind_publication_evidence(
                evidence(
                    &["item_version_001", "item_version_002"],
                    RELEASE_DIGEST,
                    "ko-KR",
                    provenance,
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(
            release
                .apply_command("publish_event", PublicationCommand::Publish, 10_200)
                .unwrap(),
            PublicationState::Published
        );
    }
}

#[test]
fn reactivation_requires_evidence_effective_at_reactivation_time() {
    let mut release = reviewed_release();
    release
        .bind_publication_evidence(
            evidence(
                &["item_version_001", "item_version_002"],
                RELEASE_DIGEST,
                "ko-KR",
                provenance(10_150, Some(10_250)).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command("publish_event", PublicationCommand::Publish, 10_200)
        .unwrap();
    release
        .apply_command("suspend_event", PublicationCommand::Suspend, 10_220)
        .unwrap();

    assert_eq!(
        release.apply_command("reactivate_event", PublicationCommand::Reactivate, 10_251,),
        Err(InstrumentReleaseError::PublicationEvidenceNotEffective)
    );
    assert_eq!(release.state(), PublicationState::Suspended);
    assert!(!release.state().accepts_new_sessions());
    assert_eq!(release.events().len(), 3);
}

#[test]
fn malformed_provenance_fails_closed() {
    assert_eq!(
        PublicationEvidenceProvenance::new(
            "sha256:not-a-digest",
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_150,
            None,
        ),
        Err(InstrumentReleaseError::InvalidEvidenceDigest)
    );
    assert_eq!(
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "12345",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_150,
            None,
        ),
        Err(InstrumentReleaseError::InvalidReference)
    );
    assert_eq!(
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            0,
            None,
        ),
        Err(InstrumentReleaseError::InvalidTimestamp)
    );
    assert_eq!(
        PublicationEvidenceProvenance::new(
            EVIDENCE_DIGEST,
            "population_general_adult_v1",
            "administration_web_self_report_v1",
            "measurement_model_big_five_v1",
            10_150,
            Some(10_149),
        ),
        Err(InstrumentReleaseError::InvalidEvidenceWindow)
    );
}

#[test]
fn malformed_record_and_wrong_binding_state_fail_closed() {
    let valid_provenance = provenance(10_050, None).unwrap();
    assert_eq!(
        evidence(&[], RELEASE_DIGEST, "ko-KR", valid_provenance.clone()),
        Err(InstrumentReleaseError::EmptyItemSet)
    );
    assert_eq!(
        evidence(
            &["item_version_001", "item_version_002"],
            "sha256:not-a-digest",
            "ko-KR",
            valid_provenance.clone(),
        ),
        Err(InstrumentReleaseError::InvalidDigest)
    );
    assert_eq!(
        evidence(
            &["item_version_001", "item_version_002"],
            RELEASE_DIGEST,
            "ko_KR",
            valid_provenance.clone(),
        ),
        Err(InstrumentReleaseError::InvalidLocale)
    );

    let approved = evidence(
        &["item_version_001", "item_version_002"],
        RELEASE_DIGEST,
        "ko-KR",
        valid_provenance,
    )
    .unwrap();
    let mut draft = InstrumentRelease::new(manifest(), 10_000).unwrap();
    assert_eq!(
        draft.bind_publication_evidence(approved),
        Err(InstrumentReleaseError::InvalidTransition)
    );

    let mut review = reviewed_release();
    assert_eq!(
        review.apply_command(
            "suspend_while_reviewing_event",
            PublicationCommand::Suspend,
            10_150,
        ),
        Err(InstrumentReleaseError::InvalidTransition)
    );
}
