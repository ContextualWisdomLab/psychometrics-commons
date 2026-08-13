//! `PostgreSQL` contract for durable participant data-rights propagation.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::data_rights::{DataRightsRequest, DataRightsRequestKind};
use psychometrics_commons_runtime::integration::IntegrationEvent;
use psychometrics_commons_runtime::postgres_data_rights::{
    apply_data_rights_migration, persist_requested_data_rights_with_propagation,
    DataRightsPersistenceDisposition, DataRightsPersistenceError, DataRightsPropagationTarget,
};
use psychometrics_commons_runtime::postgres_integration::apply_integration_migration;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn client(schema_prefix: &str) -> Client {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL is required");
    let mut client = Client::connect(&url, NoTls).expect("CI PostgreSQL must be reachable");
    let schema = format!("{schema_prefix}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .unwrap();
    client
}

fn ready_client(schema_prefix: &str) -> Client {
    let mut client = client(schema_prefix);
    apply_integration_migration(&mut client).unwrap();
    apply_data_rights_migration(&mut client).unwrap();
    client
}

fn request(scope: &str) -> DataRightsRequest {
    DataRightsRequest::new(
        "data_rights_request_alpha",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        scope,
        10_000,
    )
    .unwrap()
}

fn event() -> IntegrationEvent {
    IntegrationEvent::new(
        "data_rights_event_alpha",
        "data_rights.deletion.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_alpha",
        10_000,
        "data_rights_request_alpha",
        None,
        DIGEST,
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn custom_event(
    event_ref: &str,
    event_type: &str,
    source_ref: &str,
    tenant_ref: &str,
    subject_ref: &str,
    occurred_at_unix_ms: u64,
    correlation_ref: &str,
    causation_ref: Option<&str>,
) -> IntegrationEvent {
    IntegrationEvent::new(
        event_ref,
        event_type,
        "v1",
        source_ref,
        tenant_ref,
        subject_ref,
        occurred_at_unix_ms,
        correlation_ref,
        causation_ref,
        DIGEST,
    )
    .unwrap()
}

#[test]
fn request_target_and_outbox_commit_together_and_replay_exactly() {
    let mut db = ready_client("data_rights_exact_replay");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert_eq!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_alpha"),
            &targets,
            3,
        )
        .unwrap(),
        DataRightsPersistenceDisposition::Inserted
    );
    assert_eq!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_alpha"),
            &targets,
            3,
        )
        .unwrap(),
        DataRightsPersistenceDisposition::Duplicate
    );

    let counts: (i64, i64, i64) = (
        db.query_one("SELECT count(*) FROM data_rights_request_state", &[])
            .unwrap()
            .get(0),
        db.query_one("SELECT count(*) FROM data_rights_propagation_state", &[])
            .unwrap()
            .get(0),
        db.query_one("SELECT count(*) FROM integration_outbox", &[])
            .unwrap()
            .get(0),
    );
    assert_eq!(counts, (1, 1, 1));
}

#[test]
fn changed_request_evidence_fails_closed() {
    let mut db = ready_client("data_rights_conflicting_replay");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    persist_requested_data_rights_with_propagation(&mut db, &request("scope_alpha"), &targets, 3)
        .unwrap();

    assert!(matches!(
        persist_requested_data_rights_with_propagation(
            &mut db,
            &request("scope_beta"),
            &targets,
            3,
        ),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
}

#[test]
fn request_preconditions_fail_before_persistence() {
    let mut db = ready_client("data_rights_preconditions");
    let mut verified = request("scope_alpha");
    verified
        .verify_identity("verification_evidence_alpha", 10_100)
        .unwrap();
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &verified, &targets, 3),
        Err(DataRightsPersistenceError::InvalidRequestState)
    ));
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request("scope_alpha"), &[], 3),
        Err(DataRightsPersistenceError::EmptyTargetSet)
    ));

    let overflow = DataRightsRequest::new(
        "data_rights_request_overflow",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Deletion,
        "scope_alpha",
        u64::MAX,
    )
    .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &overflow, &targets, 3),
        Err(DataRightsPersistenceError::ValueOutOfRange)
    ));
}

#[test]
fn target_reference_and_envelope_validation_fail_closed() {
    let mut db = ready_client("data_rights_target_validation");
    let request = request("scope_alpha");
    let event = event();

    let invalid_reference = [DataRightsPropagationTarget::new("123", &event)];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &invalid_reference, 3,),
        Err(DataRightsPersistenceError::InvalidReference)
    ));

    let duplicate_targets = [
        DataRightsPropagationTarget::new("dependent_system_alpha", &event),
        DataRightsPropagationTarget::new(" dependent_system_alpha ", &event),
    ];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &duplicate_targets, 3,),
        Err(DataRightsPersistenceError::DuplicateTarget)
    ));

    let wrong_source = custom_event(
        "data_rights_event_wrong_source",
        "data_rights.deletion.requested",
        "other_bounded_context",
        "tenant_alpha",
        "data_rights_request_alpha",
        10_000,
        "data_rights_request_alpha",
        None,
    );
    let wrong_source_target = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &wrong_source,
    )];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &wrong_source_target, 3,),
        Err(DataRightsPersistenceError::InvalidPropagationEnvelope)
    ));

    let wrong_type = custom_event(
        "data_rights_event_wrong_type",
        "data_rights.export.requested",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_alpha",
        10_000,
        "data_rights_request_alpha",
        None,
    );
    let wrong_type_target = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &wrong_type,
    )];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &wrong_type_target, 3,),
        Err(DataRightsPersistenceError::InvalidPropagationEnvelope)
    ));
}

#[test]
fn remaining_envelope_fields_fail_closed() {
    let mut db = ready_client("data_rights_envelope_fields");
    let request = request("scope_alpha");
    for event in [
        custom_event(
            "data_rights_event_wrong_tenant",
            "data_rights.deletion.requested",
            "psychometrics_commons",
            "tenant_beta",
            "data_rights_request_alpha",
            10_000,
            "data_rights_request_alpha",
            None,
        ),
        custom_event(
            "data_rights_event_wrong_subject",
            "data_rights.deletion.requested",
            "psychometrics_commons",
            "tenant_alpha",
            "data_rights_request_beta",
            10_000,
            "data_rights_request_alpha",
            None,
        ),
        IntegrationEvent::new(
            "data_rights_event_wrong_schema",
            "data_rights.deletion.requested",
            "v2",
            "psychometrics_commons",
            "tenant_alpha",
            "data_rights_request_alpha",
            10_000,
            "data_rights_request_alpha",
            None,
            DIGEST,
        )
        .unwrap(),
        custom_event(
            "data_rights_event_wrong_time",
            "data_rights.deletion.requested",
            "psychometrics_commons",
            "tenant_alpha",
            "data_rights_request_alpha",
            10_001,
            "data_rights_request_alpha",
            None,
        ),
        custom_event(
            "data_rights_event_wrong_correlation",
            "data_rights.deletion.requested",
            "psychometrics_commons",
            "tenant_alpha",
            "data_rights_request_alpha",
            10_000,
            "data_rights_request_beta",
            None,
        ),
        custom_event(
            "data_rights_event_with_causation",
            "data_rights.deletion.requested",
            "psychometrics_commons",
            "tenant_alpha",
            "data_rights_request_alpha",
            10_000,
            "data_rights_request_alpha",
            Some("data_rights_cause_alpha"),
        ),
    ] {
        let mismatched = [DataRightsPropagationTarget::new(
            "dependent_system_alpha",
            &event,
        )];
        assert!(matches!(
            persist_requested_data_rights_with_propagation(&mut db, &request, &mismatched, 3),
            Err(DataRightsPersistenceError::InvalidPropagationEnvelope)
        ));
    }
}

#[test]
fn export_request_persists_export_kind() {
    let mut db = ready_client("data_rights_export");
    let request = DataRightsRequest::new(
        "data_rights_request_export",
        "tenant_alpha",
        "participant_alpha",
        DataRightsRequestKind::Export,
        "scope_export",
        12_000,
    )
    .unwrap();
    let export_event = custom_event(
        "data_rights_event_export",
        "data_rights.export.requested",
        "psychometrics_commons",
        "tenant_alpha",
        "data_rights_request_export",
        12_000,
        "data_rights_request_export",
        None,
    );
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &export_event,
    )];

    assert_eq!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3).unwrap(),
        DataRightsPersistenceDisposition::Inserted
    );
    let stored_kind: String = db
        .query_one(
            "SELECT request_kind FROM data_rights_request_state WHERE request_ref = $1",
            &[&request.request_ref()],
        )
        .unwrap()
        .get(0);
    assert_eq!(stored_kind, "export");
}

#[test]
fn outbox_validation_failure_rolls_back_local_state() {
    let mut db = ready_client("data_rights_outbox_rollback");
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 0),
        Err(DataRightsPersistenceError::Integration(_))
    ));
    let request_count: i64 = db
        .query_one("SELECT count(*) FROM data_rights_request_state", &[])
        .unwrap()
        .get(0);
    let outbox_count: i64 = db
        .query_one("SELECT count(*) FROM integration_outbox", &[])
        .unwrap()
        .get(0);
    assert_eq!((request_count, outbox_count), (0, 0));
}

#[test]
fn stored_target_count_and_identity_conflicts_fail_closed() {
    let mut count_db = ready_client("data_rights_target_count_conflict");
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut count_db, &request, &targets, 3).unwrap();
    count_db
        .execute("DELETE FROM data_rights_propagation_state", &[])
        .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut count_db, &request, &targets, 3),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));

    let mut identity_db = ready_client("data_rights_target_identity_conflict");
    persist_requested_data_rights_with_propagation(&mut identity_db, &request, &targets, 3)
        .unwrap();
    identity_db
        .execute(
            "UPDATE data_rights_propagation_state SET dependent_system_ref = $1",
            &[&"dependent_system_beta"],
        )
        .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut identity_db, &request, &targets, 3),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));

    let mut source_db = ready_client("data_rights_target_source_conflict");
    persist_requested_data_rights_with_propagation(&mut source_db, &request, &targets, 3).unwrap();
    source_db
        .batch_execute(
            "INSERT INTO integration_outbox (
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,
                 occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest,
                 max_attempts, current_state, latest_event_at_unix_ms
             )
             SELECT event_ref, event_type, schema_version, 'other_bounded_context', tenant_ref,
                    subject_ref, occurred_at_unix_ms, correlation_ref, causation_ref,
                    payload_digest, max_attempts, current_state, latest_event_at_unix_ms
             FROM integration_outbox WHERE event_ref = 'data_rights_event_alpha';
             UPDATE data_rights_propagation_state SET source_ref = 'other_bounded_context';",
        )
        .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut source_db, &request, &targets, 3),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));

    let mut event_db = ready_client("data_rights_target_event_conflict");
    persist_requested_data_rights_with_propagation(&mut event_db, &request, &targets, 3).unwrap();
    event_db
        .batch_execute(
            "INSERT INTO integration_outbox (
                 event_ref, event_type, schema_version, source_ref, tenant_ref, subject_ref,
                 occurred_at_unix_ms, correlation_ref, causation_ref, payload_digest,
                 max_attempts, current_state, latest_event_at_unix_ms
             )
             SELECT 'data_rights_event_beta', event_type, schema_version, source_ref, tenant_ref,
                    subject_ref, occurred_at_unix_ms, correlation_ref, causation_ref,
                    payload_digest, max_attempts, current_state, latest_event_at_unix_ms
             FROM integration_outbox WHERE event_ref = 'data_rights_event_alpha';
             UPDATE data_rights_propagation_state SET event_ref = 'data_rights_event_beta';",
        )
        .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut event_db, &request, &targets, 3),
        Err(DataRightsPersistenceError::ConflictingReplay)
    ));
}

#[test]
fn stored_request_field_mismatches_fail_closed() {
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    for (schema, sql) in [
        (
            "data_rights_header_tenant",
            "ALTER TABLE data_rights_propagation_state \
             DROP CONSTRAINT data_rights_propagation_request_fk; \
             UPDATE data_rights_request_state SET tenant_ref = 'tenant_beta'",
        ),
        (
            "data_rights_header_participant",
            "UPDATE data_rights_request_state SET participant_ref = 'participant_beta'",
        ),
        (
            "data_rights_header_kind",
            "UPDATE data_rights_request_state SET request_kind = 'export'",
        ),
        (
            "data_rights_header_state",
            "UPDATE data_rights_request_state SET current_state = 'processing'",
        ),
        (
            "data_rights_header_requested_at",
            "UPDATE data_rights_request_state SET requested_at_unix_ms = 9999",
        ),
        (
            "data_rights_header_latest_event",
            "UPDATE data_rights_request_state SET latest_event_at_unix_ms = 20000",
        ),
    ] {
        let mut db = ready_client(schema);
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3).unwrap();
        db.batch_execute(sql).unwrap();
        assert!(matches!(
            persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
            Err(DataRightsPersistenceError::ConflictingReplay)
        ));
    }
}

#[test]
fn missing_schema_is_a_typed_database_failure() {
    let mut db = client("data_rights_missing_schema");
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];

    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
        Err(DataRightsPersistenceError::Database(_))
    ));
}

#[test]
fn propagation_insert_failure_is_a_database_failure() {
    let mut db = ready_client("data_rights_hidden_propagation");
    db.batch_execute(
        "CREATE OR REPLACE FUNCTION data_rights_reject_propagation() \
         RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN \
             RAISE EXCEPTION 'data_rights propagation sink'; \
         END $$; \
         CREATE TRIGGER data_rights_reject_propagation \
         BEFORE INSERT ON data_rights_propagation_state \
         FOR EACH STATEMENT EXECUTE FUNCTION data_rights_reject_propagation();",
    )
    .unwrap();
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
        Err(DataRightsPersistenceError::Database(_))
    ));
}

#[test]
fn header_replay_select_failure_is_a_database_failure() {
    let mut db = ready_client("data_rights_hidden_header");
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3).unwrap();
    let sink = format!("data_rights_select_failure_sink_{}", std::process::id());
    db.batch_execute(&format!(
        "CREATE SCHEMA {sink};\
         CREATE TABLE {sink}.data_rights_request_state (\
             request_ref TEXT PRIMARY KEY\
         );\
         INSERT INTO {sink}.data_rights_request_state (request_ref) \
         VALUES ('data_rights_request_alpha');\
         SET search_path TO {sink};"
    ))
    .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
        Err(DataRightsPersistenceError::Database(_))
    ));
}

#[test]
fn target_replay_select_failure_is_a_database_failure() {
    let mut db = ready_client("data_rights_hidden_targets");
    let request = request("scope_alpha");
    let event = event();
    let targets = [DataRightsPropagationTarget::new(
        "dependent_system_alpha",
        &event,
    )];
    persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3).unwrap();
    db.batch_execute("DROP TABLE data_rights_propagation_state;")
        .unwrap();
    assert!(matches!(
        persist_requested_data_rights_with_propagation(&mut db, &request, &targets, 3),
        Err(DataRightsPersistenceError::Database(_))
    ));
}
