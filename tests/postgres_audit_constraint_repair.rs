//! Real `PostgreSQL` upgrade contract for audit-evidence constraint repair.
//!
//! A pre-existing relation can carry the expected column and constraint names while the named
//! checks themselves have been weakened. Reapplying the repository migration must restore the
//! actual product invariants, not accept names as proof of semantics.

use postgres::{Client, NoTls};
use psychometrics_commons_runtime::postgres_audit::apply_audit_evidence_migration;

const VALID_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn client() -> Client {
    let connection = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
    Client::connect(&connection, NoTls).expect("isolated CI PostgreSQL database must be reachable")
}

#[allow(clippy::too_many_arguments)]
fn insert_raw(
    client: &mut Client,
    audit_event_ref: &str,
    tenant_ref: &str,
    actor_ref: &str,
    purpose_code: &str,
    action_code: &str,
    resource_ref: &str,
    outcome_code: &str,
    evidence_digest: &str,
    occurred_at_unix_ms: i64,
) -> Result<u64, postgres::Error> {
    client.execute(
        "INSERT INTO audit_evidence_record (\
             audit_event_ref, tenant_ref, actor_ref, purpose_code, action_code, resource_ref,\
             outcome_code, evidence_digest, occurred_at_unix_ms\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        &[
            &audit_event_ref,
            &tenant_ref,
            &actor_ref,
            &purpose_code,
            &action_code,
            &resource_ref,
            &outcome_code,
            &evidence_digest,
            &occurred_at_unix_ms,
        ],
    )
}

#[test]
fn migration_reapply_restores_named_constraint_semantics() {
    let mut client = client();
    client
        .batch_execute(
            "DROP SCHEMA IF EXISTS audit_constraint_repair_test CASCADE;\
             CREATE SCHEMA audit_constraint_repair_test;\
             SET search_path TO audit_constraint_repair_test;\
             CREATE TABLE audit_evidence_record (\
                 audit_event_ref TEXT NOT NULL,\
                 tenant_ref TEXT NOT NULL,\
                 actor_ref TEXT NOT NULL,\
                 purpose_code TEXT NOT NULL,\
                 action_code TEXT NOT NULL,\
                 resource_ref TEXT NOT NULL,\
                 outcome_code TEXT NOT NULL,\
                 evidence_digest TEXT NOT NULL,\
                 occurred_at_unix_ms BIGINT NOT NULL,\
                 recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),\
                 CONSTRAINT audit_evidence_event_ref_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_tenant_ref_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_actor_ref_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_resource_ref_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_purpose_code_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_action_code_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_outcome_allowed_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_digest_shape_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_occurrence_positive_check CHECK (TRUE),\
                 CONSTRAINT audit_evidence_record_pkey PRIMARY KEY (audit_event_ref)\
             );",
        )
        .unwrap();

    apply_audit_evidence_migration(&mut client)
        .expect("migration reapply should repair compatible named constraints in place");

    let cases = [
        (
            "123",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_tenant",
            "123",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_actor",
            "tenant_alpha",
            "123",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_purpose",
            "tenant_alpha",
            "actor_alpha",
            "PurposeAlpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_action",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "ActionAlpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_resource",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "123",
            "succeeded",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_outcome",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "unknown",
            VALID_DIGEST,
            1,
        ),
        (
            "audit_invalid_digest",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            1,
        ),
        (
            "audit_invalid_time",
            "tenant_alpha",
            "actor_alpha",
            "purpose_alpha",
            "action_alpha",
            "resource_alpha",
            "succeeded",
            VALID_DIGEST,
            0,
        ),
    ];

    for (
        audit_event_ref,
        tenant_ref,
        actor_ref,
        purpose_code,
        action_code,
        resource_ref,
        outcome_code,
        evidence_digest,
        occurred_at_unix_ms,
    ) in cases
    {
        assert!(
            insert_raw(
                &mut client,
                audit_event_ref,
                tenant_ref,
                actor_ref,
                purpose_code,
                action_code,
                resource_ref,
                outcome_code,
                evidence_digest,
                occurred_at_unix_ms,
            )
            .is_err(),
            "migration reapply must restore the invariant violated by {audit_event_ref}"
        );
    }
}
