//! Domain contract for rebuilding an immutable result after process restart.

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
    let mut ledger = ResponseLedger::new("session_big_five_result").unwrap();
    ledger
        .record(
            SessionState::Active,
            ResponseWrite {
                server_event_ref: "response_event_result",
                client_event_ref: "client_event_result",
                item_version_ref: "item_version_001",
                payload_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
        )
        .unwrap();
    let response = ledger
        .freeze_as(SessionState::Completed, "response_snapshot_result")
        .unwrap();
    let request = ScoringRequest::from_snapshot(
        &response,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_result",
            response_snapshot_ref: "response_snapshot_result",
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
        "scoring_result_result",
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
            result_snapshot_ref: "result_snapshot_ko_v1",
            participant_ref: "participant_result_one",
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
fn durable_evidence_rebuilds_the_same_published_result_after_restart() {
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
fn durable_result_reconstruction_fails_closed_on_invalid_identity() {
    let published = published_snapshot();
    let mut blank = evidence_from(&published);
    blank.participant_ref = " ";
    assert_eq!(
        ResultSnapshot::from_durable_evidence(blank).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    let mut missing_consent = evidence_from(&published);
    missing_consent.consent_snapshot_refs = &[];
    assert_eq!(
        ResultSnapshot::from_durable_evidence(missing_consent).unwrap_err(),
        ResultSnapshotError::MissingConsentSnapshot
    );

    let mut zero_time = evidence_from(&published);
    zero_time.created_at_unix_ms = 0;
    assert_eq!(
        ResultSnapshot::from_durable_evidence(zero_time).unwrap_err(),
        ResultSnapshotError::InvalidCreationTime
    );

    let mut self_supersedes = evidence_from(&published);
    self_supersedes.supersedes_ref = Some("result_snapshot_ko_v1");
    assert_eq!(
        ResultSnapshot::from_durable_evidence(self_supersedes).unwrap_err(),
        ResultSnapshotError::SelfSupersession
    );

    let mut duplicate_consent = evidence_from(&published);
    let consents = vec![
        "consent_snapshot_service_v1".to_owned(),
        "consent_snapshot_service_v1".to_owned(),
    ];
    duplicate_consent.consent_snapshot_refs = &consents;
    assert_eq!(
        ResultSnapshot::from_durable_evidence(duplicate_consent).unwrap_err(),
        ResultSnapshotError::DuplicateConsentSnapshot
    );

    let mut blank_consent = evidence_from(&published);
    let blank_consents = vec![" ".to_owned()];
    blank_consent.consent_snapshot_refs = &blank_consents;
    assert_eq!(
        ResultSnapshot::from_durable_evidence(blank_consent).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    let mut blank_norm = evidence_from(&published);
    blank_norm.norm_version_ref = Some(" ");
    assert_eq!(
        ResultSnapshot::from_durable_evidence(blank_norm).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    let mut blank_supersedes = evidence_from(&published);
    blank_supersedes.supersedes_ref = Some(" ");
    assert_eq!(
        ResultSnapshot::from_durable_evidence(blank_supersedes).unwrap_err(),
        ResultSnapshotError::EmptyReference
    );

    for blank_field in [
        "result_snapshot_ref",
        "scoring_result_ref",
        "session_ref",
        "response_snapshot_ref",
        "assessment_spec_ref",
        "instrument_version_ref",
        "scoring_version_ref",
        "calibration_reference",
        "narrative_version_ref",
    ] {
        let mut blank = evidence_from(&published);
        match blank_field {
            "result_snapshot_ref" => blank.result_snapshot_ref = " ",
            "scoring_result_ref" => blank.scoring_result_ref = " ",
            "session_ref" => blank.session_ref = " ",
            "response_snapshot_ref" => blank.response_snapshot_ref = " ",
            "assessment_spec_ref" => blank.assessment_spec_ref = " ",
            "instrument_version_ref" => blank.instrument_version_ref = " ",
            "scoring_version_ref" => blank.scoring_version_ref = " ",
            "calibration_reference" => blank.calibration_reference = " ",
            "narrative_version_ref" => blank.narrative_version_ref = " ",
            _ => unreachable!("test field list is closed"),
        }
        assert_eq!(
            ResultSnapshot::from_durable_evidence(blank).unwrap_err(),
            ResultSnapshotError::EmptyReference,
            "{blank_field} must stay opaque"
        );
    }
}

#[test]
fn durable_result_reconstruction_fails_closed_on_inconsistent_scientific_evidence() {
    let published = published_snapshot();
    let mut bad_digest = evidence_from(&published);
    bad_digest.engine_artifact_digest = "sha256:not-a-digest";
    let digest_error = ResultSnapshot::from_durable_evidence(bad_digest).unwrap_err();
    assert_eq!(digest_error, ResultSnapshotError::InconsistentEvidence);
    assert_eq!(
        digest_error.to_string(),
        "durable result evidence cannot reconstruct the published snapshot"
    );

    let mut missing_prefix = evidence_from(&published);
    missing_prefix.engine_artifact_digest =
        "1111111111111111111111111111111111111111111111111111111111111111";
    assert_eq!(
        ResultSnapshot::from_durable_evidence(missing_prefix).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );

    let mut uppercase_digest = evidence_from(&published);
    uppercase_digest.engine_artifact_digest =
        "sha256:111111111111111111111111111111111111111111111111111111111111111G";
    assert_eq!(
        ResultSnapshot::from_durable_evidence(uppercase_digest).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );

    let mut empty_observations = evidence_from(&published);
    empty_observations.score_observations = Vec::new();
    assert_eq!(
        ResultSnapshot::from_durable_evidence(empty_observations).unwrap_err(),
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

    let mut zero_schema = evidence_from(&published);
    zero_schema.requested_output_schema_version = 0;
    assert_eq!(
        ResultSnapshot::from_durable_evidence(zero_schema).unwrap_err(),
        ResultSnapshotError::InconsistentEvidence
    );
}

#[test]
fn durable_result_reconstruction_accepts_absent_norm_and_supersession() {
    let published = published_snapshot();
    let mut optional_absent = evidence_from(&published);
    optional_absent.norm_version_ref = None;
    optional_absent.supersedes_ref = None;
    let rebuilt = ResultSnapshot::from_durable_evidence(optional_absent).unwrap();
    assert!(rebuilt.norm_version_ref().is_none());
    assert!(rebuilt.supersedes_ref().is_none());
}
