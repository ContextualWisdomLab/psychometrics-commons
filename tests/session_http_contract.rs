//! Public session HTTP maps create and reload onto the sealed persist start path.

use psychometrics_commons_runtime::anonymous_credential::AnonymousCredential;
use psychometrics_commons_runtime::authorization::{AuthorizationContext, ProductRole};
use psychometrics_commons_runtime::participant::ParticipantRecord;
use psychometrics_commons_runtime::postgres_assessment_session::{
    AssessmentSessionPersistenceDisposition, AssessmentSessionPersistenceError,
    AssessmentSessionStartError,
};
use psychometrics_commons_runtime::session::AssessmentSession;
use psychometrics_commons_runtime::session_http::{
    handle_authorized_session_http_request, handle_session_http_request, MemorySessionHttpPort,
    SessionHttpAuthority, SessionHttpPort, SESSION_COLLECTION_PATH,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PARTICIPANT: &str = "ptc_eb1b318917d24ca0ac5153c37ff696c7";
const RELEASE: &str = "release_big_five_ko_v1";
const VERSION: &str = "instrument_version_big_five_ko_v1";
const SESSION: &str = "ses_ipip_ko_quick_start";

fn stored_session() -> AssessmentSession {
    AssessmentSession::from_persisted_created(
        SESSION,
        PARTICIPANT,
        RELEASE,
        VERSION,
        DIGEST,
        "ko-KR",
        20_000,
    )
    .unwrap()
}

fn create_request(idempotency: &str, locale: &str) -> String {
    format!(
        "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
         Host: assessment.example\r\n\
         Idempotency-Key: {idempotency}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: 1\r\n\
         \r\n\
         {{\"participant_ref\":\"{PARTICIPANT}\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"{locale}\"}}"
    )
}

fn authorized_request<P: SessionHttpPort>(
    port: &mut P,
    request: &str,
) -> psychometrics_commons_runtime::session_http::SessionHttpResponse {
    let participant =
        ParticipantRecord::new_anonymous(PARTICIPANT, "tenant_session_http", 1).unwrap();
    let actor = AuthorizationContext::new(
        "tenant_session_http",
        "subject_session_http",
        Some(PARTICIPANT),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = SessionHttpAuthority::Authenticated(&actor);
    handle_authorized_session_http_request(request, &authority, &participant, port, 20_000)
}

fn authorized_get<P: SessionHttpPort>(
    port: &mut P,
    session_ref: &str,
) -> psychometrics_commons_runtime::session_http::SessionHttpResponse {
    authorized_request(
        port,
        &format!("GET /v1/sessions/{session_ref} HTTP/1.1\r\nHost: assessment.example\r\n\r\n"),
    )
}

#[test]
fn authorized_handler_classifies_malformed_methods_and_paths_before_loading() {
    let mut port = MemorySessionHttpPort::published();
    assert_eq!(authorized_request(&mut port, "NOT-A-REQUEST").status(), 400);
    assert_eq!(
        authorized_request(&mut port, "GET /v1/sessions HTTP/1.1\r\n\r\n").status(),
        405
    );
    assert_eq!(
        authorized_request(&mut port, "GET /v1/sessions/ HTTP/1.1\r\n\r\n").status(),
        404
    );
    assert_eq!(
        authorized_request(&mut port, "GET /v1/sessions/12 HTTP/1.1\r\n\r\n").status(),
        404
    );
    assert_eq!(
        authorized_request(&mut port, &create_request("ses_authorized_post", "ko-KR")).status(),
        201
    );
}

#[test]
fn authorized_session_read_cannot_cross_participant_ownership() {
    let mut port = MemorySessionHttpPort::published();
    let created = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(created.status(), 201);

    let participant =
        ParticipantRecord::new_anonymous("ptc_foreign_session_http", "tenant_session_http", 1)
            .unwrap();
    let actor = AuthorizationContext::new(
        "tenant_session_http",
        "subject_foreign_session_http",
        Some("ptc_foreign_session_http"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let authority = SessionHttpAuthority::Authenticated(&actor);
    let response = handle_authorized_session_http_request(
        &format!("GET /v1/sessions/{SESSION} HTTP/1.1\r\n\r\n"),
        &authority,
        &participant,
        &mut port,
        20_000,
    );
    assert_eq!(response.status(), 404);
    assert!(!response.body().contains(PARTICIPANT));
}

#[test]
fn anonymous_session_read_requires_current_credential_evidence() {
    let mut port = MemorySessionHttpPort::published();
    let created = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(created.status(), 201);
    let participant =
        ParticipantRecord::new_anonymous(PARTICIPANT, "tenant_session_http", 1).unwrap();
    let mut credential = AnonymousCredential::new(
        "anonymous_credential_session_http",
        "tenant_session_http",
        PARTICIPANT,
        SESSION,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        1_000,
        2_000,
    )
    .unwrap();
    let context = credential
        .session_context(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "tenant_session_http",
            PARTICIPANT,
            SESSION,
            1_500,
        )
        .unwrap();
    let authority = SessionHttpAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 1_500,
    };
    assert_eq!(
        handle_authorized_session_http_request(
            &format!("GET /v1/sessions/{SESSION} HTTP/1.1\r\n\r\n"),
            &authority,
            &participant,
            &mut port,
            20_000,
        )
        .status(),
        200
    );
    credential.revoke(1_600).unwrap();
    let revoked_authority = SessionHttpAuthority::Anonymous {
        context: &context,
        credential: &credential,
        now_unix_ms: 1_600,
    };
    assert_eq!(
        handle_authorized_session_http_request(
            &format!("GET /v1/sessions/{SESSION} HTTP/1.1\r\n\r\n"),
            &revoked_authority,
            &participant,
            &mut port,
            20_000,
        )
        .status(),
        404
    );
}

#[test]
fn post_creates_from_stored_release_and_public_get_requires_authority() {
    let mut port = MemorySessionHttpPort::published();
    let created = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(created.status(), 201);
    assert_eq!(created.content_type(), "application/json");
    assert!(created
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION}\"")));
    assert!(created.body().contains("\"state\":\"created\""));
    assert_eq!(port.last_start_locale.as_deref(), Some("ko-KR"));

    let replay = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.body(), created.body());

    let loaded = handle_session_http_request(
        &format!("GET /v1/sessions/{SESSION} HTTP/1.1\r\nHost: assessment.example\r\n\r\n"),
        &mut port,
        20_000,
    );
    assert_eq!(loaded.status(), 404);
    assert!(loaded.body().contains("session authority is required"));
    assert_eq!(authorized_get(&mut port, SESSION).body(), created.body());
}

#[test]
fn unpublished_catalog_replays_exact_start_and_rejects_a_new_session() {
    let mut port = MemorySessionHttpPort::published();
    let created = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(created.status(), 201);
    port.published = false;

    let replay = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(replay.status(), 200);
    assert!(replay
        .body()
        .contains(&format!("\"session_ref\":\"{SESSION}\"")));

    let rejected = handle_session_http_request(
        &create_request("ses_after_suspend", "ko-KR"),
        &mut port,
        21_000,
    );
    assert_eq!(rejected.status(), 409);
    assert_eq!(rejected.content_type(), "application/problem+json");
    assert!(rejected.body().contains("instrument-release-unavailable"));
    assert!(rejected
        .body()
        .contains("publish the exact instrument release before starting a new session"));
}

fn status(port: &mut MemorySessionHttpPort, request: &str, created_at: u64) -> u16 {
    handle_session_http_request(request, port, created_at).status()
}

#[test]
fn http_routing_failures_name_the_allowed_session_routes() {
    let mut port = MemorySessionHttpPort::published();
    assert_eq!(status(&mut port, "NOT-A-REQUEST", 20_000), 400);
    assert_eq!(
        status(&mut port, "GET /v1/sessions HTTP/1.1\r\n\r\n", 20_000),
        405
    );
    assert_eq!(
        status(&mut port, "PUT /v1/sessions HTTP/1.1\r\n\r\n", 20_000),
        405
    );
    assert_eq!(
        status(
            &mut port,
            "DELETE /v1/sessions/ses_x HTTP/1.1\r\n\r\n",
            20_000
        ),
        405
    );
    assert_eq!(
        status(&mut port, "GET /v1/results/r1 HTTP/1.1\r\n\r\n", 20_000),
        404
    );
    assert_eq!(
        status(&mut port, "POST /v1/sessions HTTP/1.1\r\n\r\n{}", 20_000),
        400
    );
    assert_eq!(
        status(
            &mut port,
            "POST /v1/sessions HTTP/1.1\r\nIdempotency-Key: 12\r\n\r\n{}",
            20_000,
        ),
        400
    );
    assert_eq!(
        status(
            &mut port,
            "GET /v1/sessions/ses_missing HTTP/1.1\r\n\r\n",
            20_000
        ),
        404
    );
    assert!(handle_session_http_request(
        "GET /v1/sessions/ses_missing HTTP/1.1\r\n\r\n",
        &mut port,
        20_000,
    )
    .body()
    .contains("session authority is required to load a stored session"));
}

#[test]
fn start_and_load_errors_map_to_buyer_actions() {
    let mut port = MemorySessionHttpPort::published();
    let locale = handle_session_http_request(&create_request(SESSION, "en-US"), &mut port, 20_000);
    assert_eq!(locale.status(), 409);
    assert!(locale.body().contains("locale-mismatch"));
    port.next_start_error = Some(AssessmentSessionStartError::InvalidReference);
    assert_eq!(
        status(&mut port, &create_request("ses_bad", "ko-KR"), 20_000),
        400
    );
    port.next_start_error = Some(AssessmentSessionStartError::InvalidTimestamp);
    assert_eq!(
        status(&mut port, &create_request("ses_clock", "ko-KR"), 0),
        500
    );
    port.next_start_error = Some(AssessmentSessionStartError::InvalidStoredRelease);
    assert_eq!(
        status(&mut port, &create_request("ses_repair", "ko-KR"), 20_000),
        409
    );
    port.next_start_error = Some(AssessmentSessionStartError::from(
        AssessmentSessionPersistenceError::ConflictingReplay,
    ));
    assert_eq!(
        status(&mut port, &create_request(SESSION, "ko-KR"), 20_000),
        409
    );
    port.next_start_error = Some(AssessmentSessionStartError::from(
        AssessmentSessionPersistenceError::UnpublishedStart,
    ));
    assert_eq!(
        status(&mut port, &create_request("ses_unpub", "ko-KR"), 20_000),
        409
    );
    port.next_start_error = Some(AssessmentSessionStartError::from(
        AssessmentSessionPersistenceError::InvalidStartRelease,
    ));
    assert_eq!(
        status(&mut port, &create_request("ses_mismatch", "ko-KR"), 20_000),
        409
    );
    port.next_start_error = Some(AssessmentSessionStartError::from(
        AssessmentSessionPersistenceError::UnsupportedIsolationLevel,
    ));
    assert_eq!(
        status(&mut port, &create_request("ses_iso", "ko-KR"), 20_000),
        500
    );
    assert_eq!(
        status(&mut port, "GET /v1/sessions/12 HTTP/1.1\r\n\r\n", 20_000),
        404
    );
    port.next_load_error = Some(AssessmentSessionPersistenceError::InvalidStoredIdentity);
    assert_eq!(authorized_get(&mut port, "ses_broken").status(), 500);
}

#[test]
fn memory_port_records_exact_replay_disposition() {
    let mut port = MemorySessionHttpPort::published();
    let first = port
        .start_from_stored_release(SESSION, PARTICIPANT, RELEASE, "ko-KR", 20_000)
        .unwrap();
    assert_eq!(first.1, AssessmentSessionPersistenceDisposition::Inserted);
    assert_eq!(first.0.session_ref(), stored_session().session_ref());
    let replay = port
        .start_from_stored_release(SESSION, PARTICIPANT, RELEASE, "ko-KR", 20_000)
        .unwrap();
    assert_eq!(replay.1, AssessmentSessionPersistenceDisposition::Duplicate);
    assert!(matches!(
        port.start_from_stored_release(
            SESSION,
            "ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            RELEASE,
            "ko-KR",
            20_000
        ),
        Err(AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay
        ))
    ));
    assert!(matches!(
        port.start_from_stored_release(
            SESSION,
            PARTICIPANT,
            "release_big_five_en_v1",
            "ko-KR",
            20_000
        ),
        Err(AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay
        ))
    ));
    assert!(matches!(
        port.start_from_stored_release(SESSION, PARTICIPANT, RELEASE, "ko-KR", 20_001),
        Err(AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay
        ))
    ));
    assert!(matches!(
        port.start_from_stored_release(SESSION, PARTICIPANT, RELEASE, "en-US", 20_000),
        Err(AssessmentSessionStartError::Persistence(
            AssessmentSessionPersistenceError::ConflictingReplay
        ))
    ));
    port.next_load_error = Some(AssessmentSessionPersistenceError::InvalidStoredIdentity);
    assert!(port.load(SESSION).is_err());
    assert!(matches!(
        port.load("12"),
        Err(AssessmentSessionPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        port.load_for_participant("12", PARTICIPANT),
        Err(AssessmentSessionPersistenceError::InvalidReference)
    ));
    assert!(matches!(
        port.load_for_participant(SESSION, "12"),
        Err(AssessmentSessionPersistenceError::InvalidReference)
    ));
    assert!(port.load(SESSION).unwrap().is_some());
    assert!(port.load("ses_missing").unwrap().is_none());
    port.next_load_error = Some(AssessmentSessionPersistenceError::InvalidReference);
    assert_eq!(authorized_get(&mut port, SESSION).status(), 400);
}

#[test]
fn reused_idempotency_key_with_a_different_participant_is_conflict() {
    let mut port = MemorySessionHttpPort::published();
    let created = handle_session_http_request(&create_request(SESSION, "ko-KR"), &mut port, 20_000);
    assert_eq!(created.status(), 201);
    let conflict = handle_session_http_request(
        &format!(
            "POST {SESSION_COLLECTION_PATH} HTTP/1.1\r\n\
             Idempotency-Key: {SESSION}\r\n\
             \r\n\
             {{\"participant_ref\":\"ptc_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"instrument_release_ref\":\"{RELEASE}\",\"locale\":\"ko-KR\"}}"
        ),
        &mut port,
        20_000,
    );
    assert_eq!(conflict.status(), 409);
    assert_eq!(conflict.content_type(), "application/problem+json");
    assert!(conflict.body().contains("idempotency-conflict"));
    assert!(conflict
        .body()
        .contains("Idempotency-Key was reused with a different session create body"));
}

#[test]
fn get_rejects_empty_and_nested_session_paths() {
    let mut port = MemorySessionHttpPort::published();
    assert_eq!(
        status(&mut port, "GET /v1/sessions/ HTTP/1.1\r\n\r\n", 20_000),
        404
    );
    assert_eq!(
        status(
            &mut port,
            "GET /v1/sessions/ses_ipip_ko_quick_start/commands HTTP/1.1\r\n\r\n",
            20_000
        ),
        404
    );
}
