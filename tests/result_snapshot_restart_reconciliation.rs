//! Restart reconstruction contract for immutable product results.
//!
//! Reload copies durable score observations and provenance only. It never calls a
//! psychometric engine or invents a replacement result after a crash.

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::result::{
    ResultSnapshot, ResultSnapshotError, ResultSnapshotEvidence, ResultSnapshotInput,
};
use psychometrics_commons_runtime::scoring::{
    ObservationDisposition, ScoreObservation, ScoringRequest, ScoringRequestInput, ScoringResult,
};
use psychometrics_commons_runtime::session::SessionState;

const ENGINE_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn published_snapshot() -> ResultSnapshot {
    let mut ledger = ResponseLedger::new("session_result_restart").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_result_restart",
                client_event_ref: "client_event_result_restart",
                item_version_ref: "item_version_result_restart",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let response = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_result_restart")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result_restart",
            response_snapshot_ref: "response_snapshot_result_restart",
            assessment_spec_ref: "assessment_spec_big_five_v1",
            instrument_version_ref: "instrument_version_big_five_ko_v1",
            scoring_version_ref: "scoring_version_big_five_v1",
            calibration_reference: "calibration_big_five_ko_v1",
            norm_version_ref: Some("norm_version_big_five_ko_v1"),
            requested_output_schema_version: 1,
        },
    )
    .unwrap();
    let scoring_result = ScoringResult::new(
        "scoring_result_result_restart",
        &request,
        ENGINE_DIGEST,
        vec![
            ScoreObservation::scored("construct_extraversion", 0.42, Some(0.08)).unwrap(),
            ScoreObservation::without_score(
                "construct_openness",
                ObservationDisposition::Abstained,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    ResultSnapshot::new(
        &request,
        &scoring_result,
        ResultSnapshotInput {
            result_snapshot_ref: "result_snapshot_restart",
            participant_ref: "participant_result_restart",
            narrative_version_ref: "narrative_version_big_five_v1",
            consent_snapshot_refs: &["consent_snapshot_service_v1"],
            created_at_unix_ms: 70_000,
            supersedes_ref: Some("result_snapshot_predecessor"),
        },
    )
    .unwrap()
}

fn evidence_from(snapshot: &ResultSnapshot) -> ResultSnapshotEvidence<'_> {
    ResultSnapshotEvidence {
        result_snapshot_ref: snapshot.result_snapshot_ref(),
        participant_ref: snapshot.participant_ref(),
        scoring_result_ref: snapshot.scoring_result_ref(),
        session_ref: snapshot.session_ref(),
        response_snapshot_ref: snapshot.response_snapshot_ref(),
        assessment_spec_ref: snapshot.assessment_spec_ref(),
        instrument_version_ref: snapshot.instrument_version_ref(),
        scoring_version_ref: snapshot.scoring_version_ref(),
        calibration_reference: snapshot.calibration_reference(),
        norm_version_ref: snapshot.norm_version_ref(),
        requested_output_schema_version: snapshot.requested_output_schema_version(),
        narrative_version_ref: snapshot.narrative_version_ref(),
        consent_snapshot_refs: snapshot.consent_snapshot_refs(),
        engine_artifact_digest: snapshot.engine_artifact_digest(),
        score_observations: snapshot.score_observations().to_vec(),
        created_at_unix_ms: snapshot.created_at_unix_ms(),
        supersedes_ref: snapshot.supersedes_ref(),
    }
}

#[test]
fn durable_evidence_rebuilds_the_exact_published_result() {
    let published = published_snapshot();
    let rebuilt = ResultSnapshot::from_durable_evidence(evidence_from(&published)).unwrap();
    assert_eq!(rebuilt, published);
    assert_eq!(rebuilt.score_observations()[0].score(), Some(0.42));
    assert_eq!(
        rebuilt.score_observations()[1].disposition(),
        ObservationDisposition::Abstained
    );
}

#[test]
fn malformed_durable_evidence_fails_closed_instead_of_inventing_a_score() {
    let published = published_snapshot();

    let mut bad_digest = evidence_from(&published);
    bad_digest.engine_artifact_digest = "sha256:not-a-digest";
    assert_eq!(
        ResultSnapshot::from_durable_evidence(bad_digest).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );

    let mut duplicate_construct = evidence_from(&published);
    duplicate_construct.score_observations = vec![
        ScoreObservation::scored("construct_extraversion", 0.42, Some(0.08)).unwrap(),
        ScoreObservation::scored("construct_extraversion", 0.11, None).unwrap(),
    ];
    assert_eq!(
        ResultSnapshot::from_durable_evidence(duplicate_construct).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );

    let mut unsupported_schema = evidence_from(&published);
    unsupported_schema.requested_output_schema_version = 2;
    assert_eq!(
        ResultSnapshot::from_durable_evidence(unsupported_schema).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );
}
