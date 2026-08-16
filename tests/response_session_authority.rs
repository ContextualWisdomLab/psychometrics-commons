//! Regression tests for server-authoritative response-ledger session binding.
//!
//! Response evidence must consult the assessment-session aggregate. Callers must
//! not be able to forge lifecycle state by passing a detached `SessionState`.

use psychometrics_commons_runtime::instrument::{
    InstrumentRelease, InstrumentReleaseManifest, PublicationCommand,
    PublicationEvidenceProvenance, PublicationEvidenceRecord, PublicationEvidenceStatus,
};
use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use psychometrics_commons_runtime::session::{AssessmentSession, SessionCommand, SessionState};

const RELEASE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVIDENCE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PAYLOAD_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn published_release() -> InstrumentRelease {
    let manifest = InstrumentReleaseManifest::new(
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
    .unwrap();
    let mut release = InstrumentRelease::new(manifest, 10_000).unwrap();
    release
        .apply_command(
            "publication_review_response_authority",
            PublicationCommand::SubmitReview,
            10_100,
        )
        .unwrap();
    release
        .bind_publication_evidence(
            PublicationEvidenceRecord::new(
                "publication_evidence_response_authority",
                "evidence_policy_self_reflection_v1",
                "release_big_five_ko_v1",
                "instrument_version_big_five_ko_v1",
                &["item_version_001", "item_version_002"],
                RELEASE_DIGEST,
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
            "publication_publish_response_authority",
            PublicationCommand::Publish,
            10_200,
        )
        .unwrap();
    release
}

fn created_session(session_ref: &str) -> AssessmentSession {
    AssessmentSession::new(
        session_ref,
        "participant_response_authority",
        &published_release(),
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn write() -> ResponseWrite<'static> {
    ResponseWrite {
        server_event_ref: "event_response_authority_001",
        client_event_ref: "client_response_authority_001",
        item_version_ref: "item_version_001",
        payload_digest: PAYLOAD_DIGEST,
    }
}

#[test]
fn created_session_cannot_be_presented_as_active_by_the_caller() {
    let session = created_session("session_response_authority");
    let mut ledger = ResponseLedger::new(session.session_ref()).unwrap();

    assert_eq!(session.state(), SessionState::Created);
    assert_eq!(
        ledger.record(SessionState::Active, write()),
        Err(WriteError::SessionNotActive(SessionState::Created)),
        "a caller must not record responses by supplying a detached Active state"
    );
    assert!(ledger.is_empty());
}

#[test]
fn active_session_cannot_be_presented_as_completed_for_snapshot_freeze() {
    let mut session = created_session("session_response_freeze_authority");
    session
        .apply_command(
            "session_activate_response_freeze_authority",
            1,
            SessionCommand::Activate,
        )
        .unwrap();
    let ledger = ResponseLedger::new(session.session_ref()).unwrap();

    assert_eq!(session.state(), SessionState::Active);
    assert_eq!(
        ledger.freeze(SessionState::Completed),
        Err(WriteError::SnapshotRequiresCompleted(SessionState::Active)),
        "a caller must not freeze a snapshot by supplying a detached Completed state"
    );
}
