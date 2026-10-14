-- Dedicated, non-assumable ownership boundary for product audit evidence.
--
-- Migration execution is an administrative deployment concern, not a runtime authority. This
-- owner-hardening migration requires a superuser executor: PostgreSQL grants a non-superuser
-- CREATEROLE identity ADMIN OPTION on roles it creates, which would let that identity later grant
-- itself SET authority over the supposedly non-assumable owner. Failing before role provisioning
-- or ownership transfer keeps that bootstrap boundary explicit and auditable.
--
-- The durable audit table and its privileged helper functions are owned by one cluster role that
-- cannot log in and must not be assumable or administrable by any non-superuser role. Product
-- runtime roles are deployment-selected rather than invented by this migration: a deployment that
-- enables audit persistence must explicitly grant its runtime role schema USAGE plus SELECT and
-- INSERT on audit_evidence_record, while withholding UPDATE, DELETE, TRUNCATE, and owner
-- membership. Runtime retention authorities separately receive EXECUTE on the bounded SECURITY
-- DEFINER routine only; they never receive owner membership. This migration is idempotent and is
-- applied after the core audit migration and again after the optional retention migration so a
-- retention-aware mutation guard is bound to the same owner.

DO $audit_evidence_owner_hardening$
DECLARE
    migration_schema TEXT := current_schema();
    audit_owner_name CONSTANT TEXT := 'psychometrics_audit_evidence_owner';
    audit_owner_oid OID;
    unsafe_assumable_role TEXT;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('psychometrics-commons:audit-evidence-owner'));

    IF NOT EXISTS (
        SELECT 1
        FROM pg_roles AS executor_role
        WHERE executor_role.rolname = current_user
          AND executor_role.rolsuper
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = '42501',
            MESSAGE = 'audit evidence owner hardening requires a superuser migration executor';
    END IF;

    SELECT role_record.oid
    INTO audit_owner_oid
    FROM pg_roles AS role_record
    WHERE role_record.rolname = audit_owner_name;

    IF audit_owner_oid IS NULL THEN
        EXECUTE format(
            'CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS',
            audit_owner_name
        );

        SELECT role_record.oid
        INTO audit_owner_oid
        FROM pg_roles AS role_record
        WHERE role_record.rolname = audit_owner_name;
    END IF;

    IF audit_owner_oid IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'dedicated audit evidence owner role could not be provisioned';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM pg_roles AS role_record
        WHERE role_record.oid = audit_owner_oid
          AND (
              role_record.rolcanlogin
              OR role_record.rolsuper
              OR role_record.rolcreatedb
              OR role_record.rolcreaterole
              OR role_record.rolreplication
              OR role_record.rolbypassrls
          )
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = '42501',
            MESSAGE = 'dedicated audit evidence owner role has unsafe login or cluster privileges';
    END IF;

    SELECT candidate_role.rolname
    INTO unsafe_assumable_role
    FROM pg_roles AS candidate_role
    WHERE candidate_role.oid <> audit_owner_oid
      AND NOT candidate_role.rolsuper
      AND (
          pg_has_role(candidate_role.oid, audit_owner_oid, 'SET')
          OR pg_has_role(candidate_role.oid, audit_owner_oid, 'USAGE')
          OR EXISTS (
              SELECT 1
              FROM pg_auth_members AS membership_record
              WHERE membership_record.roleid = audit_owner_oid
                AND membership_record.member = candidate_role.oid
                AND membership_record.admin_option
          )
      )
    ORDER BY candidate_role.rolname
    LIMIT 1;

    IF unsafe_assumable_role IS NOT NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '42501',
            MESSAGE = format(
                'dedicated audit evidence owner role is assumable or administrable by non-superuser role %I',
                unsafe_assumable_role
            );
    END IF;

    IF to_regclass(format('%I.audit_evidence_record', migration_schema)) IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record must exist before ownership hardening';
    END IF;

    EXECUTE format(
        'ALTER TABLE %I.audit_evidence_record OWNER TO %I',
        migration_schema,
        audit_owner_name
    );
    -- The dedicated owner executes the SECURITY DEFINER retention routine, so name
    -- resolution of same-schema helper functions requires USAGE on that schema. Without
    -- this grant the definer-side lookup fails closed with an undefined-function error.
    EXECUTE format(
        'GRANT USAGE ON SCHEMA %I TO %I',
        migration_schema,
        audit_owner_name
    );
    EXECUTE format(
        'ALTER FUNCTION %I.audit_evidence_reference_is_valid(TEXT) OWNER TO %I',
        migration_schema,
        audit_owner_name
    );

    IF to_regprocedure(format('%I.expire_audit_evidence_before(text,bigint)', migration_schema)) IS NOT NULL THEN
        EXECUTE format(
            'ALTER FUNCTION %I.expire_audit_evidence_before(TEXT, BIGINT) OWNER TO %I',
            migration_schema,
            audit_owner_name
        );

        EXECUTE format(
            $create_retention_guard$
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
$create_retention_guard$,
            migration_schema,
            audit_owner_name
        );
    END IF;

    IF to_regprocedure(format('%I.reject_audit_evidence_mutation()', migration_schema)) IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit mutation guard must exist before ownership hardening';
    END IF;

    EXECUTE format(
        'ALTER FUNCTION %I.reject_audit_evidence_mutation() OWNER TO %I',
        migration_schema,
        audit_owner_name
    );
END;
$audit_evidence_owner_hardening$;