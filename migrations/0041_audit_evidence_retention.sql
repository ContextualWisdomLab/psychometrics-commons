-- Deployment-authorized retention for otherwise append-only audit evidence.
--
-- This migration deliberately does not choose a retention duration. Deployments must decide the
-- tenant-scoped cutoff under their approved retention/legal-hold policy and explicitly grant
-- EXECUTE on the bounded SECURITY DEFINER routine to a maintenance authority distinct from the
-- routine owner. The owner/migration identity is not an operational retention caller. Ordinary
-- UPDATE/DELETE/TRUNCATE remains blocked by the audit mutation trigger, including when an owner
-- session manually sets the caller-settable retention GUC.

DO $audit_retention_migration$
DECLARE
    migration_schema TEXT := current_schema();
    retention_owner TEXT;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('psychometrics-commons:migration-0041'));

    IF to_regclass(format('%I.audit_evidence_record', migration_schema)) IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record must exist before migration 0041';
    END IF;

    EXECUTE format(
        $create_retention_function$
CREATE OR REPLACE FUNCTION %1$I.expire_audit_evidence_before(
    retention_tenant_ref TEXT,
    retention_cutoff_unix_ms BIGINT
)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, %1$I
AS $retention_body$
DECLARE
    deleted_count BIGINT;
    current_unix_ms BIGINT;
BEGIN
    IF session_user = current_user THEN
        RAISE EXCEPTION USING
            ERRCODE = '42501',
            MESSAGE = 'audit retention must be invoked by an explicitly granted maintenance identity distinct from the routine owner';
    END IF;

    IF retention_tenant_ref IS NULL
       OR NOT audit_evidence_reference_is_valid(retention_tenant_ref)
    THEN
        RAISE EXCEPTION USING
            ERRCODE = '22023',
            MESSAGE = 'audit retention tenant reference must be exact canonical opaque identity';
    END IF;

    IF retention_cutoff_unix_ms IS NULL OR retention_cutoff_unix_ms <= 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = '22023',
            MESSAGE = 'audit retention cutoff must be positive';
    END IF;

    current_unix_ms := floor(extract(epoch FROM transaction_timestamp()) * 1000)::BIGINT;
    IF retention_cutoff_unix_ms > current_unix_ms THEN
        RAISE EXCEPTION USING
            ERRCODE = '22023',
            MESSAGE = 'audit retention cutoff must not be in the future';
    END IF;

    PERFORM set_config('psychometrics.audit_retention_execution', 'on', true);
    BEGIN
        DELETE FROM audit_evidence_record
        WHERE tenant_ref = retention_tenant_ref
          AND occurred_at_unix_ms < retention_cutoff_unix_ms;
        GET DIAGNOSTICS deleted_count = ROW_COUNT;
    EXCEPTION WHEN OTHERS THEN
        PERFORM set_config('psychometrics.audit_retention_execution', 'off', true);
        RAISE;
    END;
    PERFORM set_config('psychometrics.audit_retention_execution', 'off', true);
    RETURN deleted_count;
END;
$retention_body$;
$create_retention_function$,
        migration_schema
    );

    EXECUTE format(
        'REVOKE ALL ON FUNCTION %I.expire_audit_evidence_before(TEXT, BIGINT) FROM PUBLIC',
        migration_schema
    );

    SELECT pg_get_userbyid(procedure_record.proowner)
    INTO retention_owner
    FROM pg_proc AS procedure_record
    WHERE procedure_record.oid = to_regprocedure(
        format('%I.expire_audit_evidence_before(text,bigint)', migration_schema)
    );

    IF retention_owner IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit retention routine owner could not be resolved';
    END IF;

    EXECUTE format(
        $create_mutation_guard$
CREATE OR REPLACE FUNCTION %1$I.reject_audit_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $mutation_guard_body$
BEGIN
    IF TG_OP = 'DELETE'
       AND current_user = %2$L
       AND session_user <> current_user
       AND current_setting('psychometrics.audit_retention_execution', true) = 'on'
    THEN
        RETURN OLD;
    END IF;

    RAISE EXCEPTION 'audit evidence is append-only'
        USING ERRCODE = '55000';
END;
$mutation_guard_body$;
$create_mutation_guard$,
        migration_schema,
        retention_owner
    );
END;
$audit_retention_migration$;