//! Regression coverage for opaque response-event reference validation.

#[path = "response_support/mod.rs"]
mod response_support;

use psychometrics_commons_runtime::response::{ResponseLedger, ResponseWrite, WriteError};
use response_support::{active_session, completed_session};

const PAYLOAD_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write<'a>(
    server_event_ref: &'a str,
    client_event_ref: &'a str,
    item_version_ref: &'a str,
) -> ResponseWrite<'a> {
    ResponseWrite {
        server_event_ref,
        client_event_ref,
        item_version_ref,
        payload_digest: PAYLOAD_DIGEST,
    }
}

#[test]
fn response_ledger_session_reference_must_be_opaque() {
    for session_ref in ["", "   ", "12345", "1.25e3", "１２３４５"] {
        assert_eq!(
            ResponseLedger::new(session_ref),
            Err(WriteError::InvalidReference)
        );
    }
}

#[test]
fn response_ledger_accepts_an_exact_opaque_session_reference() {
    assert!(ResponseLedger::new("session_ref").is_ok());
}

#[test]
fn response_identity_references_reject_numeric_like_values() {
    let session = active_session("session_ref");
    for request in [
        write("12345", "client_event_a", "item_version_a"),
        write("server_event_a", "1.25e3", "item_version_a"),
        write("server_event_a", "client_event_a", "１２３４５"),
    ] {
        let mut ledger = ResponseLedger::from_session(&session).unwrap();
        assert_eq!(
            ledger.record(&session, request),
            Err(WriteError::InvalidReference)
        );
        assert!(ledger.is_empty());
    }
}

#[test]
fn response_identity_references_reject_surrounding_whitespace_aliases() {
    for session_ref in [
        " session_ref ",
        "\u{00a0}session_ref\u{00a0}",
        "\u{2003}session_ref\u{2003}",
        "\u{202f}session_ref\u{202f}",
        "\u{3000}session_ref\u{3000}",
    ] {
        assert_eq!(
            ResponseLedger::new(session_ref),
            Err(WriteError::InvalidReference)
        );
    }

    let session = active_session("session_ref");
    for request in [
        write(" server_event_a ", "client_event_a", "item_version_a"),
        write(
            "server_event_a",
            "\u{00a0}client_event_a\u{00a0}",
            "item_version_a",
        ),
        write(
            "server_event_a",
            "client_event_a",
            "\u{2003}item_version_a\u{2003}",
        ),
    ] {
        let mut ledger = ResponseLedger::from_session(&session).unwrap();
        assert_eq!(
            ledger.record(&session, request),
            Err(WriteError::InvalidReference)
        );
        assert!(ledger.is_empty());
    }

    let completed = completed_session("session_ref");
    let ledger = ResponseLedger::from_session(&completed).unwrap();
    for snapshot_ref in [
        " snapshot_ref_a ",
        "\u{202f}snapshot_ref_a\u{202f}",
        "\u{3000}snapshot_ref_a\u{3000}",
    ] {
        assert_eq!(
            ledger.freeze_as(&completed, snapshot_ref),
            Err(WriteError::InvalidReference)
        );
    }

    assert_eq!(
        WriteError::InvalidReference.to_string(),
        "response identity references must use exact opaque spelling without surrounding whitespace"
    );
}

#[test]
fn response_identity_references_preserve_exact_visible_spelling() {
    let session = active_session("세션_가나다");
    let mut ledger = ResponseLedger::from_session(&session).unwrap();
    let original = ledger
        .record(
            &session,
            write(
                "서버_사건_가나다",
                "클라이언트_사건_가나다",
                "문항_버전_가나다",
            ),
        )
        .unwrap();

    assert_eq!(original.server_event_ref(), "서버_사건_가나다");
    assert_eq!(original.client_event_ref(), "클라이언트_사건_가나다");
    assert_eq!(original.item_version_ref(), "문항_버전_가나다");

    let completed = completed_session("세션_가나다");
    let replay = ledger
        .record(
            &completed,
            write(
                "무시되는_서버_참조",
                "클라이언트_사건_가나다",
                "문항_버전_가나다",
            ),
        )
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(ledger.len(), 1);

    let snapshot = ledger.freeze_as(&completed, "응답_스냅샷_가나다").unwrap();
    assert_eq!(snapshot.snapshot_ref(), Some("응답_스냅샷_가나다"));
    assert_eq!(snapshot.session_ref(), "세션_가나다");
}
