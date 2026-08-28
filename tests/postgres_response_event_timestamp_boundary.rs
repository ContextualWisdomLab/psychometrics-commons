//! Real `PostgreSQL` boundary contract for response-event timestamps.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_response_event::{
    apply_response_event_migration, persist_response_event, ResponseEventPersistenceDisposition,
    ResponseEventPersistenceError,
};
use psychometrics_commons_runtime::response::ResponseEvent;

const SCHEMA: &str = "response_event_timestamp_boundary_test";
const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// PostgreSQL's timestamp upper bound is exclusive 294277-01-01. At the
// response-event API's whole-millisecond precision, this is the final valid
// Unix millisecond and the immediately following millisecond must fail before
// a database write is attempted.
const POSTGRES_MAX_WHOLE_MILLISECOND_UNIX_MS: u64 = 9_223_372_277_884_799;
const FIRST_INVALID_POSTGRES_MILLISECOND_UNIX_MS: u64 = POSTGRES_MAX_WHOLE_MILLISECOND_UNIX_MS + 1;

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

fn event() -> ResponseEvent {
    ResponseEvent::from_persisted(
        "server_event_timestamp_boundary",
        "client_event_timestamp_boundary",
        "item_version_timestamp_boundary",
        DIGEST,
        1,
    )
    .expect("fixture event must satisfy the response-event domain contract")
}

#[test]
fn postgres_timestamp_upper_boundary_is_classified_before_write() {
    let mut client = test_client();
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;\
             CREATE SCHEMA {SCHEMA};\
             SET search_path TO {SCHEMA};"
        ))
        .expect("isolated timestamp-boundary schema must be resettable");
    apply_response_event_migration(&mut client).expect("response-event migration must apply");

    let event = event();
    let mut transaction = client.transaction().expect("transaction must start");
    assert_eq!(
        persist_response_event(
            &mut transaction,
            "session_timestamp_boundary_valid",
            &event,
            POSTGRES_MAX_WHOLE_MILLISECOND_UNIX_MS,
            POSTGRES_MAX_WHOLE_MILLISECOND_UNIX_MS,
        )
        .expect("the final PostgreSQL whole millisecond must remain representable"),
        ResponseEventPersistenceDisposition::Inserted
    );
    transaction
        .rollback()
        .expect("boundary fixture transaction must roll back");

    let mut transaction = client.transaction().expect("transaction must start");
    assert!(matches!(
        persist_response_event(
            &mut transaction,
            "session_timestamp_boundary_invalid",
            &event,
            FIRST_INVALID_POSTGRES_MILLISECOND_UNIX_MS,
            FIRST_INVALID_POSTGRES_MILLISECOND_UNIX_MS,
        ),
        Err(ResponseEventPersistenceError::InvalidTimestamp)
    ));
    transaction
        .rollback()
        .expect("invalid-boundary transaction must roll back");
}
