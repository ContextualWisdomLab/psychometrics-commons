//! Regression tests for server-authoritative item-delivery session binding.
//!
//! Item-delivery evidence must be bound to the exact immutable release pinned by
//! the assessment session, and callers must not be able to forge lifecycle state
//! by passing a detached `SessionState` value.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::item_delivery::{ItemDeliveryLedger, ItemDeliveryRequest};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionState};

const RELEASE_A_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RELEASE_B_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EVIDENCE_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn manifest(
    release_ref: &str,
    instrument_version_ref: &str,
    content_digest: &str,
) -> InstrumentReleaseManifest {
    InstrumentReleaseManifest::new(
        release_ref,
        "instrument_big_five",
        instrument_version_ref,
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
        content_digest,
    )
    .unwrap()
}

fn published_release() -> InstrumentRelease {
    let mut release = InstrumentRelease::new(
        manifest(
            "release_big_five_ko_v1",
            "instrument_version_big_five_ko_v1",
            RELEASE_A_DIGEST,
        ),
        10_000,
    )
    .unwrap();
    release
        .apply_command(
            "publication_review_session_authority",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_session_authority",
                "evidence_policy_self_reflection_v1",
                "release_big_five_ko_v1",
                "instrument_version_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
                RELEASE_A_DIGEST,
                "ko-KR",
                "intended_use_self_reflection_v1",
                "assessment_spec_big_five_v1",
                "scoring_version_big_five_v1",
                "calibration_big_five_ko_v1",
                Some("norm_version_big_five_ko_v1"),
                "limitations_nonclinical_v1",
                PublicationEvidenceProvenance::new(
                    EVIDENCE_DIGEST,
                    "population_general_adult_v1",
                    "administration_web_self_report_v1",
                    "measurement_model_big_five_v1",
                    10_050,
                    None,
                )
                .unwrap(),
                &["rights_ipip_big_five_v1"],
                &["recovery_big_five_ko_v1"],
                &["approval_psychometrics_big_five_ko_v1"],
                PublicationEvidenceStatus::Approved,
            )
            .unwrap(),
        )
        .unwrap();
    release
        .apply_command(
            "publication_publish_session_authority",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn created_session(release: &InstrumentRelease) -> AssessmentSession {
    AssessmentSession::new(
        "session_item_delivery_authority",
        "participant_item_delivery_authority",
        release,
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn delivery_request() -> ItemDeliveryRequest<'static> {
    ItemDeliveryRequest {
        delivery_ref: "delivery_session_authority_001",
        item_version_ref: "item_version_001",
        presentation_context_ref: "presentation_standard_v1",
        selection_evidence_ref: None,
    }
}

#[test]
fn ledger_rejects_manifest_not_pinned_by_the_assessment_session() {
    let release = published_release();
    let session = created_session(&release);
    let other_manifest = manifest(
        "release_big_five_ko_v2",
        "instrument_version_big_five_ko_v2",
        RELEASE_B_DIGEST,
    );

    assert!(
        ItemDeliveryLedger::from_manifest(session.session_ref(), &other_manifest).is_err(),
        "a caller-controlled session reference must not bind a ledger to another release"
    );
}

#[test]
fn caller_cannot_forge_active_state_for_a_created_session() {
    let release = published_release();
    let session = created_session(&release);
    let mut ledger = ItemDeliveryLedger::from_manifest(session.session_ref(), release.manifest())
        .expect("matching manifest should create the ledger");

    assert_eq!(session.state(), SessionState::Created);
    assert!(
        ledger
            .deliver(SessionState::Active, delivery_request())
            .is_err(),
        "detached lifecycle state must not override the authoritative session aggregate"
    );
    assert!(ledger.is_empty());
}
