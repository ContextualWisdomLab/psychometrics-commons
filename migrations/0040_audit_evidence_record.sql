-- Append-only product audit evidence.
--
-- `occurred_at_unix_ms` is the server-observed action time carried by the domain record.
-- `recorded_at` is independent database system-recorded time so operators can distinguish event
-- time from durable receipt time during incident review.
-- PostgreSQL 18's pg_unicode_fast collation keeps direct-SQL reference guards aligned with the
-- product domain's Unicode whitespace and numeric-like opacity boundary.

DO $audit_evidence_schema$
DECLARE
    relation_ref REGCLASS := to_regclass('audit_evidence_record');
    created_table BOOLEAN := relation_ref IS NULL;
    actual_columns TEXT[];
    actual_defaults TEXT[];
    actual_constraint_names TEXT[];
    actual_constraints TEXT[];
    actual_constraint_manifest TEXT;
    stored_constraint_manifest TEXT;
    constraint_manifest_prefix CONSTANT TEXT :=
        'psychometrics-commons:migration-0040:constraint-manifest:';
BEGIN
    IF created_table THEN
        EXECUTE $create_audit_evidence_record$
CREATE TABLE audit_evidence_record (
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
    CONSTRAINT audit_evidence_event_ref_shape_check CHECK (
        audit_event_ref <> ''
        AND audit_event_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            audit_event_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND audit_event_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT audit_evidence_tenant_ref_shape_check CHECK (
        tenant_ref <> ''
        AND tenant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            tenant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND tenant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT audit_evidence_actor_ref_shape_check CHECK (
        actor_ref <> ''
        AND actor_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            actor_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND actor_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT audit_evidence_resource_ref_shape_check CHECK (
        resource_ref <> ''
        AND resource_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            resource_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND resource_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
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
)
$create_audit_evidence_record$;
        relation_ref := to_regclass('audit_evidence_record');
    END IF;

    IF relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record migration did not create its owned table';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s',
            attribute.attname,
            format_type(attribute.atttypid, attribute.atttypmod),
            CASE WHEN attribute.attnotnull THEN 'not_null' ELSE 'nullable' END
        )
        FROM pg_attribute AS attribute
        WHERE attribute.attrelid = relation_ref
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
        ORDER BY attribute.attnum
    ) INTO actual_columns;

    IF actual_columns IS DISTINCT FROM ARRAY[
        'audit_event_ref:text:not_null',
        'tenant_ref:text:not_null',
        'actor_ref:text:not_null',
        'purpose_code:text:not_null',
        'action_code:text:not_null',
        'resource_ref:text:not_null',
        'outcome_code:text:not_null',
        'evidence_digest:text:not_null',
        'occurred_at_unix_ms:bigint:not_null',
        'recorded_at:timestamp with time zone:not_null'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record column contract does not match migration 0040';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            attribute.attname,
            pg_get_expr(default_value.adbin, default_value.adrelid)
        )
        FROM pg_attribute AS attribute
        JOIN pg_attrdef AS default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        WHERE attribute.attrelid = relation_ref
        ORDER BY attribute.attnum
    ) INTO actual_defaults;

    IF actual_defaults IS DISTINCT FROM ARRAY[
        'recorded_at:transaction_timestamp()'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record default contract does not match migration 0040';
    END IF;

    SELECT ARRAY(
        SELECT constraint_record.conname::TEXT
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraint_names;

    IF actual_constraint_names IS DISTINCT FROM ARRAY[
        'audit_evidence_action_code_shape_check',
        'audit_evidence_actor_ref_shape_check',
        'audit_evidence_digest_shape_check',
        'audit_evidence_event_ref_shape_check',
        'audit_evidence_occurrence_positive_check',
        'audit_evidence_outcome_allowed_check',
        'audit_evidence_purpose_code_shape_check',
        'audit_evidence_record_pkey',
        'audit_evidence_resource_ref_shape_check',
        'audit_evidence_tenant_ref_shape_check'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record constraint contract does not match migration 0040';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s:%s:%s',
            constraint_record.conname,
            constraint_record.contype,
            constraint_record.convalidated,
            constraint_record.conenforced,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraints;

    actual_constraint_manifest := array_to_string(actual_constraints, E'\n');

    IF created_table THEN
        EXECUTE format(
            'COMMENT ON TABLE %s IS %L',
            relation_ref,
            constraint_manifest_prefix || actual_constraint_manifest
        );
    ELSE
        stored_constraint_manifest := obj_description(relation_ref, 'pg_class');
        IF stored_constraint_manifest IS NULL
            OR left(stored_constraint_manifest, char_length(constraint_manifest_prefix))
                IS DISTINCT FROM constraint_manifest_prefix
            OR substring(
                stored_constraint_manifest
                FROM char_length(constraint_manifest_prefix) + 1
            ) IS DISTINCT FROM actual_constraint_manifest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '55000',
                MESSAGE = 'audit_evidence_record constraint contract does not match migration 0040';
        END IF;
    END IF;
END
$audit_evidence_schema$;

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
