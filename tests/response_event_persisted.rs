//! Reconstruct a mid-session response ledger after process restart.
//!
//! A buyer on the Korean IPIP Quick path can answer item 1, lose the process,
//! and still freeze the same scoring prefix after item 2. Persistence adapters
//! must rebuild that ledger without inventing answers or scores.

#[path = "response_support/mod.rs"]
mod response_support;

use response_support::{active_session, completed_session};

fn frozen_session() -> psychometrics_commons_runtime::session::AssessmentSession {
    completed_session("session_ipip_ko_quick")
}

use psychometrics_commons_runtime::response::{ResponseEvent, ResponseLedger, ResponseWrite};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};

const DIGEST_N1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_N2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
    payload_digest: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest,
    }
}

fn scoring_input<'a>() -> ScoringRequestInput<'a> {
    ScoringRequestInput {
        scoring_request_ref: "scoring_request_ipip_ko_quick",
        response_snapshot_ref: "response_snapshot_ipip_ko_quick",
        assessment_spec_ref: "assessment_spec_ipip_bf_ko_quick",
        instrument_version_ref: "instrument_version_ipip_bf_ko_quick",
        scoring_version_ref: "scoring_version_ipip_mlsirm_v1",
        calibration_reference: "calibration_ipip_bf_ko_quick",
        norm_version_ref: Some("norm_ipip_bf_ko_quick"),
        requested_output_schema_version: 1,
    }
}

#[test]
fn reconstructed_two_item_korean_path_pins_the_same_scoring_request() {
    let session_live = active_session("session_ipip_ko_quick");
    let mut live = ResponseLedger::from_session(&session_live).unwrap();
    live.record(
        &session_live,
        write(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
        ),
    )
    .unwrap();
    live.record(
        &session_live,
        write(
            "server_event_item_02",
            "client_event_item_02",
            "item_version_n2_ko",
            DIGEST_N2,
        ),
    )
    .unwrap();
    let expected_snapshot = live
        .freeze_as(&frozen_session(), "response_snapshot_ipip_ko_quick")
        .unwrap();
    let expected_request =
        ScoringRequest::from_snapshot(&expected_snapshot, scoring_input()).unwrap();

    let rebuilt = ResponseLedger::from_persisted(
        "session_ipip_ko_quick",
        vec![
            ResponseEvent::from_persisted(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
                1,
            )
            .unwrap(),
            ResponseEvent::from_persisted(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
                2,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let rebuilt_snapshot = rebuilt
        .freeze_as(&frozen_session(), "response_snapshot_ipip_ko_quick")
        .unwrap();
    let rebuilt_request =
        ScoringRequest::from_snapshot(&rebuilt_snapshot, scoring_input()).unwrap();

    assert_eq!(rebuilt.session_ref(), "session_ipip_ko_quick");
    assert_eq!(rebuilt.events(), live.events());
    assert_eq!(rebuilt_snapshot, expected_snapshot);
    assert_eq!(rebuilt_request, expected_request);
    assert_eq!(
        rebuilt_request.response_snapshot_ref(),
        "response_snapshot_ipip_ko_quick"
    );
}

#[test]
fn restarted_korean_path_records_item_two_and_keeps_the_scoring_prefix() {
    let session_control = active_session("session_ipip_ko_quick");
    let mut control = ResponseLedger::from_session(&session_control).unwrap();
    control
        .record(
            &session_control,
            write(
                "server_event_item_01",
                "client_event_item_01",
                "item_version_n1_ko",
                DIGEST_N1,
            ),
        )
        .unwrap();
    control
        .record(
            &session_control,
            write(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
            ),
        )
        .unwrap();
    let expected_snapshot = control
        .freeze_as(&frozen_session(), "response_snapshot_ipip_ko_quick")
        .unwrap();
    let expected_request =
        ScoringRequest::from_snapshot(&expected_snapshot, scoring_input()).unwrap();

    let mut after_restart = ResponseLedger::from_persisted(
        "session_ipip_ko_quick",
        vec![ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
            1,
        )
        .unwrap()],
    )
    .unwrap();
    after_restart
        .record(
            &session_control,
            write(
                "server_event_item_02",
                "client_event_item_02",
                "item_version_n2_ko",
                DIGEST_N2,
            ),
        )
        .unwrap();
    let rebuilt_snapshot = after_restart
        .freeze_as(&frozen_session(), "response_snapshot_ipip_ko_quick")
        .unwrap();
    let rebuilt_request =
        ScoringRequest::from_snapshot(&rebuilt_snapshot, scoring_input()).unwrap();

    assert_eq!(after_restart.events(), control.events());
    assert_eq!(rebuilt_snapshot, expected_snapshot);
    assert_eq!(rebuilt_request, expected_request);
}

#[test]
fn persisted_event_reconstruction_fails_closed_on_identity_and_sequence() {
    assert!(matches!(
        ResponseEvent::from_persisted(
            " ",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            "server_event_item_01",
            "12",
            "item_version_n1_ko",
            DIGEST_N1,
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            " server_event_item_01 ",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            " ",
            DIGEST_N1,
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "   ",
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::EmptyReference)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            "sha256:not-a-digest",
            1
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidPayloadDigest)
    ));
    assert!(matches!(
        ResponseEvent::from_persisted(
            "server_event_item_01",
            "client_event_item_01",
            "item_version_n1_ko",
            DIGEST_N1,
            0
        ),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidSequence)
    ));
}

#[test]
fn persisted_ledger_reconstruction_fails_closed_on_gaps_and_duplicate_identities() {
    let first = ResponseEvent::from_persisted(
        "server_event_item_01",
        "client_event_item_01",
        "item_version_n1_ko",
        DIGEST_N1,
        1,
    )
    .unwrap();
    let gapped = ResponseEvent::from_persisted(
        "server_event_item_03",
        "client_event_item_03",
        "item_version_n3_ko",
        DIGEST_N2,
        3,
    )
    .unwrap();
    assert!(matches!(
        ResponseLedger::from_persisted(" ", vec![]),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseLedger::from_persisted(" session_ipip_ko_quick ", vec![]),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidReference)
    ));
    assert!(matches!(
        ResponseLedger::from_persisted("session_ipip_ko_quick", vec![gapped.clone()]),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidSequence)
    ));
    assert!(matches!(
        ResponseLedger::from_persisted("session_ipip_ko_quick", vec![first.clone(), gapped]),
        Err(psychometrics_commons_runtime::response::WriteError::InvalidSequence)
    ));

    let duplicate_server = ResponseEvent::from_persisted(
        "server_event_item_01",
        "client_event_item_02",
        "item_version_n2_ko",
        DIGEST_N2,
        2,
    )
    .unwrap();
    assert!(matches!(
        ResponseLedger::from_persisted(
            "session_ipip_ko_quick",
            vec![first.clone(), duplicate_server]
        ),
        Err(psychometrics_commons_runtime::response::WriteError::ServerReferenceConflict)
    ));

    let duplicate_client = ResponseEvent::from_persisted(
        "server_event_item_02",
        "client_event_item_01",
        "item_version_n2_ko",
        DIGEST_N2,
        2,
    )
    .unwrap();
    assert!(matches!(
        ResponseLedger::from_persisted("session_ipip_ko_quick", vec![first, duplicate_client]),
        Err(psychometrics_commons_runtime::response::WriteError::IdempotencyConflict)
    ));

    let empty = ResponseLedger::from_persisted("session_ipip_ko_quick", vec![]).unwrap();
    assert_eq!(empty.session_ref(), "session_ipip_ko_quick");
    assert!(empty.events().is_empty());
}
