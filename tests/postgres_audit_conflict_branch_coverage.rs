//! Branch-complete conflict coverage for immutable PostgreSQL audit replay classification.
//!
//! The persistence classifier compares each immutable field in a deliberate fail-closed order.
//! Every comparison therefore needs evidence for both the matching and conflicting path; otherwise
//! a later field could regress without the exact owned-production branch-coverage gate noticing.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::audit::{AuditEvidence, AuditEvidenceInput, AuditOutcome};
use psychometrics_commons_runtime::postgres_audit::{
    apply_audit_evidence_migration, persist_audit_evidence, AuditPersistenceError,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str =
    "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const BASE_TIME: u64 = 1_785_000_000_000;

#[derive(Clone, Copy)]
struct EvidenceFields<'a> {
    tenant_ref: &'a str,
    actor_ref: &'a str,
    purpose_code: &'a str,
    action_code: &'a str,
    resource_ref: &'a str,
    outcome: AuditOutcome,
    evidence_digest: &'a str,
    occurred_at_unix_ms: u64,
}

impl Default for EvidenceFields<'static> {
    fn default() -> Self {
        Self {
            tenant_ref: "tenant_research_alpha",
            actor_ref: "actor_publisher_alpha",
            purpose_code: "instrument_publication",
            action_code: "publish_instrument_release",
            resource_ref: "instrument_release_big_five_ko_v1",
            outcome: AuditOutcome::Succeeded,
            evidence_digest: DIGEST,
            occurred_at_unix_ms: BASE_TIME,
        }
    }
}

fn evidence(event_ref: &str, fields: EvidenceFields<'_>) -> AuditEvidence {
    AuditEvidence::new(AuditEvidenceInput {
        audit_event_ref: event_ref,
        tenant_ref: fields.tenant_ref,
        actor_ref: fields.actor_ref,
        purpose_code: fields.purpose_code,
        action_code: fields.action_code,
        resource_ref: fields.resource_ref,
        outcome: fields.outcome,
        evidence_digest: fields.evidence_digest,
        occurred_at_unix_ms: fields.occurred_at_unix_ms,
    })
    .expect("branch-coverage fixture must be valid audit evidence")
}

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    let mut client = Client::connect(&connection, NoTls)
        .expect("isolated CI PostgreSQL database must be reachable");
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_conflict_branch_coverage_test CASCADE;\
             CREATE SCHEMA audit_conflict_branch_coverage_test;\
             SET search_path TO audit_conflict_branch_coverage_test;",
        )
        .expect("isolated conflict-coverage schema must be created");
    apply_audit_evidence_migration(&mut client)
        .expect("audit evidence migration must apply in the isolated schema");
    client
}

fn assert_conflicting_replay(client: &mut Client, event_ref: &str, conflict: EvidenceFields<'_>) {
    let original = evidence(event_ref, EvidenceFields::default());
    {
        let mut transaction = client.transaction().expect("insert transaction must start");
        persist_audit_evidence(&mut transaction, &original)
            .expect("baseline audit evidence must persist");
        transaction.commit().expect("baseline insert must commit");
    }

    let conflicting = evidence(event_ref, conflict);
    let mut transaction = client
        .transaction()
        .expect("conflict transaction must start");
    assert!(matches!(
        persist_audit_evidence(&mut transaction, &conflicting),
        Err(AuditPersistenceError::ConflictingReplay)
    ));
    transaction
        .rollback()
        .expect("conflicting replay transaction must roll back");
}

#[test]
fn every_immutable_field_mismatch_fails_closed() {
    let mut client = client();

    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_tenant",
        EvidenceFields {
            tenant_ref: "tenant_research_beta",
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_actor",
        EvidenceFields {
            actor_ref: "actor_publisher_beta",
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_purpose",
        EvidenceFields {
            purpose_code: "research_release_access",
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_action",
        EvidenceFields {
            action_code: "suspend_instrument_release",
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_resource",
        EvidenceFields {
            resource_ref: "instrument_release_big_five_en_v1",
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_outcome",
        EvidenceFields {
            outcome: AuditOutcome::Denied,
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_digest",
        EvidenceFields {
            evidence_digest: OTHER_DIGEST,
            ..EvidenceFields::default()
        },
    );
    assert_conflicting_replay(
        &mut client,
        "audit_event_conflict_timestamp",
        EvidenceFields {
            occurred_at_unix_ms: BASE_TIME + 1,
            ..EvidenceFields::default()
        },
    );
}
