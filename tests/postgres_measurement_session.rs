//! Real `PostgreSQL` persist → process-death → reload contract.
//!
//! The buyer continues from stored consent, audit, membership, and export
//! pointer evidence. This slice does not persist scores or identity-link history.

use postgres::{Client, IsolationLevel, NoTls};
use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::consent::{ConsentDecision, ConsentPurpose};
use psychometrics_commons_runtime::measurement_session::{
    ExportSnapshotPointer, MeasurementSession, MeasurementSessionError, MeasurementSessionInput,
    SessionAuditEvent, SessionConsentRecord, SessionEncryptionKey, SessionMembership,
    MEASUREMENT_SESSION_PERSIST_PURPOSE,
};
use psychometrics_commons_runtime::postgres_measurement_session::{
    apply_measurement_session_migration, load_measurement_session, persist_measurement_session,
    MeasurementSessionPersistenceDisposition, MeasurementSessionPersistenceError,
};
use std::error::Error;
use std::sync::{Mutex, MutexGuard};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn test_client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS measurement_session_persist_test;\
             SET search_path TO measurement_session_persist_test;",
        )
        .unwrap();
    client
}

fn reset_tables(client: &mut Client) {
    client
        .batch_execute(
            "DROP TABLE IF EXISTS measurement_session_persist_test.export_snapshot_pointer;\
             DROP TABLE IF EXISTS measurement_session_persist_test.session_audit_event;\
             DROP TABLE IF EXISTS measurement_session_persist_test.session_consent_record;\
             DROP TABLE IF EXISTS measurement_session_persist_test.session_membership;\
             DROP TABLE IF EXISTS measurement_session_persist_test.measurement_session;\
             DROP TABLE IF EXISTS measurement_session_persist_test.assessment_participant;",
        )
        .unwrap();
}

fn encryption_key() -> SessionEncryptionKey {
    SessionEncryptionKey::new(MEASUREMENT_SESSION_PERSIST_PURPOSE, [3_u8; 32]).unwrap()
}

fn other_key() -> SessionEncryptionKey {
    SessionEncryptionKey::new(MEASUREMENT_SESSION_PERSIST_PURPOSE, [4_u8; 32]).unwrap()
}

fn actor() -> AuthorizationContext {
    AuthorizationContext::new(
        "tenant_alpha",
        "subject_alpha",
        Some("participant_alpha"),
        &[ProductRole::Participant],
    )
    .unwrap()
}

fn assemble(input: MeasurementSessionInput) -> MeasurementSession {
    MeasurementSession::new(input).unwrap()
}

fn live_session() -> MeasurementSession {
    assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: vec![
            SessionMembership::new(
                "participant_alpha",
                "tenant_alpha",
                1_699_000_000_000,
                1_700_000_000_100,
            )
            .unwrap(),
            SessionMembership::new(
                "participant_beta",
                "tenant_alpha",
                1_699_000_000_200,
                1_700_000_000_200,
            )
            .unwrap(),
        ],
        consent_records: vec![
            SessionConsentRecord::new(
                "consent_service",
                "participant_alpha",
                ConsentPurpose::ServiceOperation,
                ConsentDecision::Granted,
                "consent_form_service_v1",
                None,
                1_700_000_000_300,
            )
            .unwrap(),
            SessionConsentRecord::new(
                "consent_research",
                "participant_alpha",
                ConsentPurpose::ResearchContribution,
                ConsentDecision::Granted,
                "consent_form_research_v1",
                Some("research_scope_alpha"),
                1_700_000_000_400,
            )
            .unwrap(),
        ],
        audit_events: vec![SessionAuditEvent::new(
            "audit_enroll",
            "actor_alpha",
            "session_enroll",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            DIGEST,
            1_700_000_000_500,
        )
        .unwrap()],
        export_snapshot_pointer: Some(
            ExportSnapshotPointer::new(
                "snapshot_alpha",
                "request_alpha",
                DIGEST,
                1_700_000_000_600,
            )
            .unwrap(),
        ),
    })
}

fn persist(
    client: &mut Client,
    session: &MeasurementSession,
) -> Result<MeasurementSessionPersistenceDisposition, MeasurementSessionPersistenceError> {
    let mut transaction = client.transaction().unwrap();
    let disposition =
        persist_measurement_session(&mut transaction, &actor(), session, &encryption_key());
    match &disposition {
        Ok(_) => transaction.commit().unwrap(),
        Err(_) => transaction.rollback().unwrap(),
    }
    disposition
}

#[test]
fn persist_then_process_death_reloads_two_audit_events_in_canonical_order() {
    let _guard = test_guard();
    let mut writer = test_client();
    reset_tables(&mut writer);
    apply_measurement_session_migration(&mut writer).unwrap();
    let original = assemble(MeasurementSessionInput {
        session_ref: "session_dual_audit".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 61,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 62, 63).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: vec![
            SessionAuditEvent::new(
                "audit_zeta",
                "actor_alpha",
                "session_export",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                OTHER_DIGEST,
                65,
            )
            .unwrap(),
            SessionAuditEvent::new(
                "audit_alpha",
                "actor_alpha",
                "session_enroll",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                DIGEST,
                64,
            )
            .unwrap(),
        ],
        export_snapshot_pointer: None,
    });
    persist(&mut writer, &original).unwrap();
    drop(writer);

    let mut reader = test_client();
    let mut transaction = reader.transaction().unwrap();
    let restored = load_measurement_session(
        &mut transaction,
        &actor(),
        "session_dual_audit",
        &encryption_key(),
    )
    .unwrap()
    .expect("two audit events must reload after process death");
    transaction.commit().unwrap();
    assert_eq!(restored, original);
    assert_eq!(restored.provenance_bytes(), original.provenance_bytes());
    assert_eq!(
        restored.audit_events()[0].event_ref(),
        "audit_alpha",
        "reload must restore canonical audit order"
    );
    assert_eq!(restored.audit_events()[1].event_ref(), "audit_zeta");
}

#[test]
fn persist_then_process_death_reloads_consent_audit_and_membership() {
    let _guard = test_guard();
    let mut writer = test_client();
    reset_tables(&mut writer);
    apply_measurement_session_migration(&mut writer).unwrap();
    let original = live_session();
    assert_eq!(
        persist(&mut writer, &original).unwrap(),
        MeasurementSessionPersistenceDisposition::Inserted
    );
    drop(writer);

    let mut reader = test_client();
    let mut transaction = reader.transaction().unwrap();
    let restored = load_measurement_session(
        &mut transaction,
        &actor(),
        original.session_ref(),
        &encryption_key(),
    )
    .unwrap()
    .expect("a persisted live session must reload after the writer process dies");
    transaction.commit().unwrap();

    assert_eq!(restored, original);
    assert_eq!(restored.provenance_bytes(), original.provenance_bytes());
    assert_eq!(restored.provenance_digest(), original.provenance_digest());
    assert_eq!(restored.consent_records(), original.consent_records());
    assert_eq!(restored.audit_events(), original.audit_events());
    assert_eq!(restored.memberships(), original.memberships());
    assert_eq!(
        restored.export_snapshot_pointer(),
        original.export_snapshot_pointer()
    );
    assert!(
        restored.service_operation_is_granted("participant_alpha"),
        "buyer must continue after reload without re-consenting"
    );
}

#[test]
fn exact_replay_is_duplicate_and_rebinding_fails_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let original = live_session();
    persist(&mut client, &original).unwrap();
    assert_eq!(
        persist(&mut client, &original).unwrap(),
        MeasurementSessionPersistenceDisposition::Duplicate
    );

    let rebound_created = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_001,
        memberships: original.memberships().to_vec(),
        consent_records: original.consent_records().to_vec(),
        audit_events: original.audit_events().to_vec(),
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_created).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_owner = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_beta".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: original.consent_records().to_vec(),
        audit_events: original.audit_events().to_vec(),
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    let beta_actor = AuthorizationContext::new(
        "tenant_alpha",
        "subject_beta",
        Some("participant_beta"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let owner_conflict = persist_measurement_session(
        &mut transaction,
        &beta_actor,
        &rebound_owner,
        &encryption_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        owner_conflict,
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn unauthorized_or_wrong_key_cannot_read_or_rewrite_the_session() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let original = live_session();
    persist(&mut client, &original).unwrap();

    let foreign = AuthorizationContext::new(
        "tenant_beta",
        "subject_alpha",
        Some("participant_alpha"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let persist_denied =
        persist_measurement_session(&mut transaction, &foreign, &original, &encryption_key())
            .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        persist_denied,
        MeasurementSessionPersistenceError::Unauthorized(AuthorizationError::CrossTenantDenied)
    ));
    assert!(persist_denied.source().is_some());

    let mut transaction = client.transaction().unwrap();
    let load_denied = load_measurement_session(
        &mut transaction,
        &foreign,
        original.session_ref(),
        &encryption_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        load_denied,
        MeasurementSessionPersistenceError::Unauthorized(AuthorizationError::CrossTenantDenied)
    ));

    let mut transaction = client.transaction().unwrap();
    let wrong_key = load_measurement_session(
        &mut transaction,
        &actor(),
        original.session_ref(),
        &other_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        wrong_key,
        MeasurementSessionPersistenceError::Domain(_)
    ));
    assert!(!wrong_key.to_string().is_empty());
    assert!(wrong_key.source().is_some());
}

#[test]
fn missing_session_and_invalid_reload_references_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(load_measurement_session(
        &mut transaction,
        &actor(),
        "session_missing",
        &encryption_key(),
    )
    .unwrap()
    .is_none());
    let invalid =
        load_measurement_session(&mut transaction, &actor(), "12", &encryption_key()).unwrap_err();
    assert!(matches!(
        invalid,
        MeasurementSessionPersistenceError::InvalidReference
    ));
    assert!(!invalid.to_string().is_empty());
    assert!(invalid.source().is_none());
    for error in [
        MeasurementSessionPersistenceError::InvalidReference,
        MeasurementSessionPersistenceError::ConflictingReplay,
        MeasurementSessionPersistenceError::ValueOutOfRange,
        MeasurementSessionPersistenceError::UnsupportedIsolationLevel,
        MeasurementSessionPersistenceError::from(AuthorizationError::CrossTenantDenied),
        MeasurementSessionPersistenceError::from(MeasurementSessionError::SealingFailed),
    ] {
        assert!(!error.to_string().is_empty());
        let _ = error.source();
    }
    transaction.commit().unwrap();
}

#[test]
fn isolation_and_missing_relation_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let original = live_session();
    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        persist_measurement_session(&mut serializable, &actor(), &original, &encryption_key())
            .unwrap_err(),
        MeasurementSessionPersistenceError::UnsupportedIsolationLevel
    ));
    serializable.rollback().unwrap();

    let mut serializable = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut serializable,
            &actor(),
            original.session_ref(),
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::UnsupportedIsolationLevel
    ));
    serializable.rollback().unwrap();

    reset_tables(&mut client);
    let mut transaction = client.transaction().unwrap();
    let missing =
        persist_measurement_session(&mut transaction, &actor(), &original, &encryption_key())
            .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        missing,
        MeasurementSessionPersistenceError::Database(_)
    ));
    assert!(!missing.to_string().is_empty());
    assert!(missing.source().is_some());

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let mut aborted = client.transaction().unwrap();
    assert!(aborted
        .batch_execute("SELECT 1 FROM measurement_session_persist_test.relation_does_not_exist")
        .is_err());
    assert!(matches!(
        persist_measurement_session(&mut aborted, &actor(), &original, &encryption_key())
            .unwrap_err(),
        MeasurementSessionPersistenceError::Database(_)
    ));
    aborted.rollback().unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn overflow_timestamp_and_pointer_conflicts_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let overflow = assemble(MeasurementSessionInput {
        session_ref: "session_overflow".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: u64::MAX,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 20).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    assert!(matches!(
        persist(&mut client, &overflow).unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    let overflow_member_created = assemble(MeasurementSessionInput {
        session_ref: "session_overflow_member".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 12,
        memberships: vec![SessionMembership::new(
            "participant_alpha",
            "tenant_alpha",
            u64::MAX,
            20,
        )
        .unwrap()],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    assert!(matches!(
        persist(&mut client, &overflow_member_created).unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    let overflow_enrolled = assemble(MeasurementSessionInput {
        session_ref: "session_overflow_enrolled".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 13,
        memberships: vec![SessionMembership::new(
            "participant_alpha",
            "tenant_alpha",
            10,
            u64::MAX,
        )
        .unwrap()],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    assert!(matches!(
        persist(&mut client, &overflow_enrolled).unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    let overflow_audit = assemble(MeasurementSessionInput {
        session_ref: "session_overflow_audit".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 15,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 17).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: vec![SessionAuditEvent::new(
            "audit_overflow",
            "actor_alpha",
            "session_enroll",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            DIGEST,
            u64::MAX,
        )
        .unwrap()],
        export_snapshot_pointer: None,
    });
    assert!(matches!(
        persist(&mut client, &overflow_audit).unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    let overflow_export = assemble(MeasurementSessionInput {
        session_ref: "session_overflow_export".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 18,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 19).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: Some(
            ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", DIGEST, u64::MAX)
                .unwrap(),
        ),
    });
    assert!(matches!(
        persist(&mut client, &overflow_export).unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));

    let without_pointer = assemble(MeasurementSessionInput {
        session_ref: "session_pointer".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 11,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 20).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    persist(&mut client, &without_pointer).unwrap();
    assert_eq!(
        persist(&mut client, &without_pointer).unwrap(),
        MeasurementSessionPersistenceDisposition::Duplicate
    );
    let with_pointer = assemble(MeasurementSessionInput {
        session_ref: "session_pointer".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 11,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 20).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: Some(
            ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", DIGEST, 50).unwrap(),
        ),
    });
    persist(&mut client, &with_pointer).unwrap();
    let rebound_pointer = assemble(MeasurementSessionInput {
        session_ref: "session_pointer".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 11,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 10, 20).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: Some(
            ExportSnapshotPointer::new("snapshot_beta", "request_alpha", OTHER_DIGEST, 50).unwrap(),
        ),
    });
    assert!(matches!(
        persist(&mut client, &rebound_pointer).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
    assert!(matches!(
        persist(&mut client, &without_pointer).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn membership_consent_audit_and_participant_rebinding_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let original = live_session();
    persist(&mut client, &original).unwrap();

    let rebound_member = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 1_699_000_000_000, 99)
                .unwrap(),
            SessionMembership::new(
                "participant_beta",
                "tenant_alpha",
                1_699_000_000_200,
                1_700_000_000_200,
            )
            .unwrap(),
        ],
        consent_records: original.consent_records().to_vec(),
        audit_events: original.audit_events().to_vec(),
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_member).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_participant = assemble(MeasurementSessionInput {
        session_ref: "session_gamma".to_owned(),
        tenant_ref: "tenant_beta".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 12,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_beta", 13, 14).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    let foreign_actor = AuthorizationContext::new(
        "tenant_beta",
        "subject_alpha",
        Some("participant_alpha"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let mut transaction = client.transaction().unwrap();
    let conflict = persist_measurement_session(
        &mut transaction,
        &foreign_actor,
        &rebound_participant,
        &encryption_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        conflict,
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_consent = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: vec![SessionConsentRecord::new(
            "consent_service",
            "participant_alpha",
            ConsentPurpose::ServiceOperation,
            ConsentDecision::Revoked,
            "consent_form_service_v1",
            None,
            1_700_000_000_300,
        )
        .unwrap()],
        audit_events: original.audit_events().to_vec(),
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_consent).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_audit = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: original.consent_records().to_vec(),
        audit_events: vec![SessionAuditEvent::new(
            "audit_enroll",
            "actor_beta",
            "session_enroll",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            OTHER_DIGEST,
            1_700_000_000_500,
        )
        .unwrap()],
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_audit).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_audit_time = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: original.consent_records().to_vec(),
        audit_events: vec![SessionAuditEvent::new(
            "audit_enroll",
            "actor_alpha",
            "session_enroll",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            DIGEST,
            1_700_000_000_501,
        )
        .unwrap()],
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_audit_time).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
}

#[test]
fn reload_restores_a_session_without_an_export_pointer() {
    let _guard = test_guard();
    let mut writer = test_client();
    reset_tables(&mut writer);
    apply_measurement_session_migration(&mut writer).unwrap();
    let original = assemble(MeasurementSessionInput {
        session_ref: "session_bare".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 21,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 22, 23).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    persist(&mut writer, &original).unwrap();
    drop(writer);

    let mut reader = test_client();
    let mut transaction = reader.transaction().unwrap();
    let restored = load_measurement_session(
        &mut transaction,
        &actor(),
        "session_bare",
        &encryption_key(),
    )
    .unwrap()
    .expect("a session without an export pointer must still reload");
    transaction.commit().unwrap();
    assert_eq!(restored.provenance_bytes(), original.provenance_bytes());
    assert!(restored.export_snapshot_pointer().is_none());
}

#[test]
fn load_maps_a_missing_relation_to_a_database_error() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    let mut transaction = client.transaction().unwrap();
    let error = load_measurement_session(
        &mut transaction,
        &actor(),
        "session_alpha",
        &encryption_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    assert!(matches!(
        error,
        MeasurementSessionPersistenceError::Database(_)
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn later_missing_export_relation_and_field_rebinding_fail_closed() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let without_pointer = assemble(MeasurementSessionInput {
        session_ref: "session_export_drop".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 31,
        memberships: vec![
            SessionMembership::new("participant_alpha", "tenant_alpha", 32, 33).unwrap(),
        ],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    persist(&mut client, &without_pointer).unwrap();
    client
        .batch_execute("DROP TABLE measurement_session_persist_test.export_snapshot_pointer;")
        .unwrap();
    assert!(matches!(
        persist(&mut client, &without_pointer).unwrap_err(),
        MeasurementSessionPersistenceError::Database(_)
    ));

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    let original = live_session();
    persist(&mut client, &original).unwrap();

    let created_at_conflict = assemble(MeasurementSessionInput {
        session_ref: "session_created_member".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 41,
        memberships: vec![SessionMembership::new(
            "participant_alpha",
            "tenant_alpha",
            1_699_000_000_001,
            42,
        )
        .unwrap()],
        consent_records: Vec::new(),
        audit_events: Vec::new(),
        export_snapshot_pointer: None,
    });
    assert!(matches!(
        persist(&mut client, &created_at_conflict).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    client
        .execute(
            "UPDATE measurement_session_persist_test.measurement_session \
             SET tenant_ref = 'tenant_gamma' WHERE session_ref = $1",
            &[&original.session_ref()],
        )
        .unwrap();
    assert!(matches!(
        persist(&mut client, &original).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
    client
        .execute(
            "UPDATE measurement_session_persist_test.measurement_session \
             SET tenant_ref = 'tenant_alpha' WHERE session_ref = $1",
            &[&original.session_ref()],
        )
        .unwrap();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &original).unwrap();
    client
        .batch_execute(
            "UPDATE measurement_session_persist_test.session_audit_event \
             SET encryption_nonce = '\\x000000000000000000000000'::bytea \
             WHERE event_ref = 'audit_enroll';",
        )
        .unwrap();
    assert!(matches!(
        persist(&mut client, &original).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &original).unwrap();

    let rebound_consent_member = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: vec![SessionConsentRecord::new(
            "consent_service",
            "participant_beta",
            ConsentPurpose::ServiceOperation,
            ConsentDecision::Granted,
            "consent_form_service_v1",
            None,
            1_700_000_000_300,
        )
        .unwrap()],
        audit_events: original.audit_events().to_vec(),
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_consent_member).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    let rebound_audit_digest = assemble(MeasurementSessionInput {
        session_ref: "session_alpha".to_owned(),
        tenant_ref: "tenant_alpha".to_owned(),
        owner_participant_ref: "participant_alpha".to_owned(),
        created_at_unix_ms: 1_700_000_000_000,
        memberships: original.memberships().to_vec(),
        consent_records: original.consent_records().to_vec(),
        audit_events: vec![SessionAuditEvent::new(
            "audit_enroll",
            "actor_alpha",
            "session_enroll",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            OTHER_DIGEST,
            1_700_000_000_500,
        )
        .unwrap()],
        export_snapshot_pointer: original.export_snapshot_pointer().cloned(),
    });
    assert!(matches!(
        persist(&mut client, &rebound_audit_digest).unwrap_err(),
        MeasurementSessionPersistenceError::ConflictingReplay
    ));

    for pointer in [
        ExportSnapshotPointer::new("snapshot_beta", "request_alpha", DIGEST, 1_700_000_000_600)
            .unwrap(),
        ExportSnapshotPointer::new("snapshot_alpha", "request_beta", DIGEST, 1_700_000_000_600)
            .unwrap(),
        ExportSnapshotPointer::new(
            "snapshot_alpha",
            "request_alpha",
            OTHER_DIGEST,
            1_700_000_000_600,
        )
        .unwrap(),
        ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", DIGEST, 1_700_000_000_601)
            .unwrap(),
    ] {
        let rebound = assemble(MeasurementSessionInput {
            session_ref: "session_alpha".to_owned(),
            tenant_ref: "tenant_alpha".to_owned(),
            owner_participant_ref: "participant_alpha".to_owned(),
            created_at_unix_ms: 1_700_000_000_000,
            memberships: original.memberships().to_vec(),
            consent_records: original.consent_records().to_vec(),
            audit_events: original.audit_events().to_vec(),
            export_snapshot_pointer: Some(pointer),
        });
        assert!(matches!(
            persist(&mut client, &rebound).unwrap_err(),
            MeasurementSessionPersistenceError::ConflictingReplay
        ));
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn corrupt_stored_evidence_fails_closed_on_reload() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();

    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.session_audit_event \
             DROP CONSTRAINT session_audit_event_occurred_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.session_audit_event \
             SET occurred_at_unix_ms = -1 WHERE event_ref = 'audit_enroll';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.session_audit_event \
             DROP CONSTRAINT session_audit_event_encryption_nonce_check;\
             UPDATE measurement_session_persist_test.session_audit_event \
             SET encryption_nonce = '\\x00'::bytea WHERE event_ref = 'audit_enroll';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::Domain(_)
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.session_membership \
             DROP CONSTRAINT session_membership_enrolled_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.session_membership \
             SET enrolled_at_unix_ms = -1 WHERE participant_ref = 'participant_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.assessment_participant \
             DROP CONSTRAINT assessment_participant_created_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.assessment_participant \
             SET created_at_unix_ms = -1 WHERE participant_ref = 'participant_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.assessment_participant \
             DROP CONSTRAINT assessment_participant_created_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.assessment_participant \
             SET created_at_unix_ms = 0 WHERE participant_ref = 'participant_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.assessment_participant \
             DROP CONSTRAINT assessment_participant_tenant_ref_check;\
             UPDATE measurement_session_persist_test.assessment_participant \
             SET tenant_ref = '12' WHERE participant_ref = 'participant_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::InvalidReference
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.session_consent_record \
             DROP CONSTRAINT session_consent_record_encryption_nonce_check;\
             UPDATE measurement_session_persist_test.session_consent_record \
             SET encryption_nonce = '\\x00'::bytea WHERE event_ref = 'consent_service';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::Domain(_)
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.session_audit_event \
             DROP CONSTRAINT session_audit_event_ciphertext_payload_check;\
             UPDATE measurement_session_persist_test.session_audit_event \
             SET ciphertext_payload = '\\x00'::bytea WHERE event_ref = 'audit_enroll';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::Domain(_)
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.export_snapshot_pointer \
             DROP CONSTRAINT export_snapshot_pointer_content_digest_check;\
             UPDATE measurement_session_persist_test.export_snapshot_pointer \
             SET content_digest = 'md5:00' WHERE session_ref = 'session_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::Domain(_)
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.export_snapshot_pointer \
             DROP CONSTRAINT export_snapshot_pointer_created_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.export_snapshot_pointer \
             SET created_at_unix_ms = -1 WHERE session_ref = 'session_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "ALTER TABLE measurement_session_persist_test.measurement_session \
             DROP CONSTRAINT measurement_session_created_at_unix_ms_check;\
             UPDATE measurement_session_persist_test.measurement_session \
             SET created_at_unix_ms = -1 WHERE session_ref = 'session_alpha';",
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::ValueOutOfRange
    ));
    transaction.rollback().unwrap();

    client
        .batch_execute("DELETE FROM measurement_session_persist_test.session_membership;")
        .unwrap();
    client
        .execute(
            "UPDATE measurement_session_persist_test.measurement_session \
             SET created_at_unix_ms = 1700000000000 WHERE session_ref = 'session_alpha'",
            &[],
        )
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    assert!(matches!(
        load_measurement_session(
            &mut transaction,
            &actor(),
            "session_alpha",
            &encryption_key(),
        )
        .unwrap_err(),
        MeasurementSessionPersistenceError::Domain(_)
    ));
    transaction.rollback().unwrap();
}

fn load_after_dropping(table: &str) -> MeasurementSessionPersistenceError {
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(&format!(
            "DROP TABLE measurement_session_persist_test.{table} CASCADE;"
        ))
        .unwrap();
    let mut transaction = client.transaction().unwrap();
    let error = load_measurement_session(
        &mut transaction,
        &actor(),
        "session_alpha",
        &encryption_key(),
    )
    .unwrap_err();
    transaction.rollback().unwrap();
    error
}

#[test]
fn reload_fails_closed_when_a_later_relation_is_dropped() {
    let _guard = test_guard();
    for table in [
        "session_membership",
        "session_consent_record",
        "session_audit_event",
        "export_snapshot_pointer",
    ] {
        assert!(
            matches!(
                load_after_dropping(table),
                MeasurementSessionPersistenceError::Database(_)
            ),
            "reload after dropping {table} must fail closed"
        );
    }
}

#[test]
fn exact_replay_fails_closed_when_conflict_select_cannot_read() {
    let _guard = test_guard();
    let mut client = test_client();
    reset_tables(&mut client);
    apply_measurement_session_migration(&mut client).unwrap();
    persist(&mut client, &live_session()).unwrap();
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS measurement_session_select_sink;\
             CREATE OR REPLACE FUNCTION measurement_session_redirect_search_path() \
             RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 PERFORM set_config('search_path', 'measurement_session_select_sink', true); \
                 RETURN NEW; \
             END $$; \
             CREATE TRIGGER assessment_participant_redirect_search_path \
             BEFORE INSERT ON measurement_session_persist_test.assessment_participant \
             FOR EACH ROW EXECUTE FUNCTION measurement_session_redirect_search_path();",
        )
        .unwrap();
    assert!(matches!(
        persist(&mut client, &live_session()).unwrap_err(),
        MeasurementSessionPersistenceError::Database(_)
    ));
}
