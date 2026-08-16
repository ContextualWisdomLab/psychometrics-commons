//! Durable snapshot reconstruction must match the freeze that scoring will consume.

use psychometrics_commons_runtime::response::{
    ResponseLedger, ResponseSnapshot, ResponseSnapshotEntryInput, ResponseWrite, WriteError,
};
use psychometrics_commons_runtime::scoring::{ScoringRequest, ScoringRequestInput};
use psychometrics_commons_runtime::session::SessionState;

const PAYLOAD_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

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

fn frozen_two_item_snapshot() -> ResponseSnapshot {
    let mut ledger = ResponseLedger::new("session_reload_alpha").unwrap();
    ledger
        .record(
            SessionState::Active,
            write(
                "server_event_zzz_first",
                "client_event_001",
                "item_version_001",
                PAYLOAD_DIGEST,
            ),
        )
        .unwrap();
    ledger
        .record(
            SessionState::Active,
            write(
                "server_event_aaa_second",
                "client_event_002",
                "item_version_002",
                OTHER_DIGEST,
            ),
        )
        .unwrap();
    ledger
        .freeze_as(SessionState::Completed, "response_snapshot_reload_alpha")
        .unwrap()
}

#[test]
fn persisted_entries_rebuild_the_same_snapshot_scoring_can_dispatch() {
    let frozen = frozen_two_item_snapshot();
    let rebuilt = ResponseSnapshot::from_persisted(
        "response_snapshot_reload_alpha",
        "session_reload_alpha",
        &[
            ResponseSnapshotEntryInput {
                event_ref: "server_event_zzz_first",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            },
            ResponseSnapshotEntryInput {
                event_ref: "server_event_aaa_second",
                item_version_ref: "item_version_002",
                payload_digest: OTHER_DIGEST,
            },
        ],
        Some(2),
    )
    .expect("stored completed prefix must reconstruct");

    assert_eq!(rebuilt, frozen);
    let request = ScoringRequest::from_snapshot(
        &rebuilt,
        ScoringRequestInput {
            scoring_request_ref: "scoring_request_reload_alpha",
            response_snapshot_ref: "response_snapshot_reload_alpha",
            assessment_spec_ref: "assessment_spec_reload_alpha",
            instrument_version_ref: "instrument_version_reload_alpha",
            scoring_version_ref: "scoring_version_reload_alpha",
            calibration_reference: "calibration_reload_alpha",
            norm_version_ref: Some("norm_version_reload_alpha"),
            requested_output_schema_version: 1,
        },
    )
    .expect("a reloaded completed snapshot must remain scoreable");
    assert_eq!(
        request.response_snapshot_ref(),
        "response_snapshot_reload_alpha"
    );
    assert_eq!(request.session_ref(), "session_reload_alpha");
}

#[test]
fn persisted_reconstruction_keeps_server_order_not_event_identity_order() {
    let rebuilt = ResponseSnapshot::from_persisted(
        "response_snapshot_reload_order",
        "session_reload_order",
        &[
            ResponseSnapshotEntryInput {
                event_ref: "server_event_zzz_first",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            },
            ResponseSnapshotEntryInput {
                event_ref: "server_event_aaa_second",
                item_version_ref: "item_version_002",
                payload_digest: OTHER_DIGEST,
            },
        ],
        Some(2),
    )
    .unwrap();

    assert_eq!(
        rebuilt.event_refs(),
        ["server_event_zzz_first", "server_event_aaa_second"]
    );
    assert_eq!(rebuilt.last_sequence(), Some(2));
}

#[test]
fn persisted_reconstruction_rejects_blank_refs_bad_digests_and_sequence_lies() {
    let valid = [ResponseSnapshotEntryInput {
        event_ref: "server_event_001",
        item_version_ref: "item_version_001",
        payload_digest: PAYLOAD_DIGEST,
    }];

    assert_eq!(
        ResponseSnapshot::from_persisted(" ", "session_reload_guard", &valid, Some(1)).unwrap_err(),
        WriteError::InvalidReference
    );
    assert_eq!(
        ResponseSnapshot::from_persisted("response_snapshot_guard", "42", &valid, Some(1))
            .unwrap_err(),
        WriteError::InvalidReference
    );
    assert_eq!(
        ResponseSnapshot::from_persisted(
            "response_snapshot_guard",
            "session_reload_guard",
            &[ResponseSnapshotEntryInput {
                event_ref: "42",
                item_version_ref: "item_version_001",
                payload_digest: PAYLOAD_DIGEST,
            }],
            Some(1),
        )
        .unwrap_err(),
        WriteError::InvalidReference
    );
    assert_eq!(
        ResponseSnapshot::from_persisted(
            "response_snapshot_guard",
            "session_reload_guard",
            &[ResponseSnapshotEntryInput {
                event_ref: "server_event_001",
                item_version_ref: "item_version_001",
                payload_digest: "sha256:not-a-digest",
            }],
            Some(1),
        )
        .unwrap_err(),
        WriteError::InvalidPayloadDigest
    );
    assert_eq!(
        ResponseSnapshot::from_persisted(
            "response_snapshot_guard",
            "session_reload_guard",
            &[ResponseSnapshotEntryInput {
                event_ref: "server_event_001",
                item_version_ref: "item_version_001",
                payload_digest: "   ",
            }],
            Some(1),
        )
        .unwrap_err(),
        WriteError::EmptyReference
    );
    assert_eq!(
        ResponseSnapshot::from_persisted(
            "response_snapshot_guard",
            "session_reload_guard",
            &valid,
            Some(3),
        )
        .unwrap_err(),
        WriteError::CorruptSnapshotEvidence
    );
    assert_eq!(
        ResponseSnapshot::from_persisted(
            "response_snapshot_empty",
            "session_reload_empty",
            &[],
            Some(1),
        )
        .unwrap_err(),
        WriteError::CorruptSnapshotEvidence
    );
    let empty = ResponseSnapshot::from_persisted(
        "response_snapshot_empty",
        "session_reload_empty",
        &[],
        None,
    )
    .unwrap();
    assert_eq!(empty.event_count(), 0);
    assert_eq!(empty.last_sequence(), None);
}
