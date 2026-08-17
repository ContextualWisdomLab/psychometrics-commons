-- Append-only product audit evidence.
--
-- `occurred_at_unix_ms` is the server-observed action time carried by the domain record.
-- `recorded_at` is independent database system-recorded time so operators can distinguish event
-- time from durable receipt time during incident review.

CREATE TABLE IF NOT EXISTS audit_evidence_record (
    audit_event_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    purpose_code TEXT NOT NULL,
    action_code TEXT NOT NULL,
    resource_ref TEXT NOT NULL,
    outcome_code TEXT NOT NULL,
    evidence_digest TEXT NOT NULL,
    occurred_at_unix_ms BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CONSTRAINT audit_evidence_event_ref_shape_check
        CHECK (audit_event_ref = btrim(audit_event_ref) AND length(audit_event_ref) > 0),
    CONSTRAINT audit_evidence_tenant_ref_shape_check
        CHECK (tenant_ref = btrim(tenant_ref) AND length(tenant_ref) > 0),
    CONSTRAINT audit_evidence_actor_ref_shape_check
        CHECK (actor_ref = btrim(actor_ref) AND length(actor_ref) > 0),
    CONSTRAINT audit_evidence_resource_ref_shape_check
        CHECK (resource_ref = btrim(resource_ref) AND length(resource_ref) > 0),
    CONSTRAINT audit_evidence_purpose_code_shape_check
        CHECK (purpose_code ~ '^[a-z][a-z0-9_]*$'),
    CONSTRAINT audit_evidence_action_code_shape_check
        CHECK (action_code ~ '^[a-z][a-z0-9_]*$'),
    CONSTRAINT audit_evidence_outcome_allowed_check
        CHECK (outcome_code IN ('succeeded', 'denied', 'failed')),
    CONSTRAINT audit_evidence_digest_shape_check
        CHECK (evidence_digest ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT audit_evidence_occurrence_positive_check
        CHECK (occurred_at_unix_ms > 0)
);

CREATE INDEX IF NOT EXISTS audit_evidence_tenant_time_index
    ON audit_evidence_record (tenant_ref, occurred_at_unix_ms, audit_event_ref);

CREATE OR REPLACE FUNCTION reject_audit_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit evidence is append-only'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS audit_evidence_reject_row_mutation ON audit_evidence_record;
CREATE TRIGGER audit_evidence_reject_row_mutation
BEFORE UPDATE OR DELETE ON audit_evidence_record
FOR EACH ROW
EXECUTE FUNCTION reject_audit_evidence_mutation();

DROP TRIGGER IF EXISTS audit_evidence_reject_truncate ON audit_evidence_record;
CREATE TRIGGER audit_evidence_reject_truncate
BEFORE TRUNCATE ON audit_evidence_record
FOR EACH STATEMENT
EXECUTE FUNCTION reject_audit_evidence_mutation();
