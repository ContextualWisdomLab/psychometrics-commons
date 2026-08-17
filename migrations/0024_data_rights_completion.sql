ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completion_evidence_ref TEXT;

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completed_at_unix_ms BIGINT;

-- Reapplication must replace an earlier revision of this not-yet-released constraint, not merely
-- trust its name. PostgreSQL 18's pg_unicode_fast collation keeps the physical opaque-reference
-- boundary aligned with Rust's Unicode whitespace and numeric-like reference guard.
ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completion_evidence_ref_format_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completion_evidence_ref_format_check
    CHECK (
        completion_evidence_ref IS NULL
        OR (
            completion_evidence_ref <> ''
            AND completion_evidence_ref COLLATE "pg_unicode_fast"
                !~ '(^[[:space:]])|([[:space:]]$)'
            AND NOT (
                completion_evidence_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
                AND completion_evidence_ref COLLATE "pg_unicode_fast"
                    ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
            )
        )
    );

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completed_time_positive_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completed_time_positive_check
            CHECK (completed_at_unix_ms IS NULL OR completed_at_unix_ms > 0);
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_presence_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_presence_check
            CHECK ((completion_evidence_ref IS NULL) = (completed_at_unix_ms IS NULL));
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_state_evidence_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_state_evidence_check
            CHECK (
                (current_state IN ('completed', 'partially_completed'))
                = (completion_evidence_ref IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
            );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_after_processing_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_after_processing_check
            CHECK (
                completed_at_unix_ms IS NULL
                OR (
                    processing_started_at_unix_ms IS NOT NULL
                    AND completed_at_unix_ms >= processing_started_at_unix_ms
                )
            );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_scope_fk_unique'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_scope_fk_unique
            UNIQUE (request_ref, tenant_ref, request_kind, current_state);
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS data_rights_retained_scope_evidence (
    request_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    request_kind TEXT NOT NULL DEFAULT 'deletion',
    completion_state TEXT NOT NULL DEFAULT 'partially_completed',
    retained_scope_ref TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (request_ref, retained_scope_ref),
    CONSTRAINT data_rights_retained_scope_request_fk
        FOREIGN KEY (request_ref, tenant_ref, request_kind, completion_state)
        REFERENCES data_rights_request_state
            (request_ref, tenant_ref, request_kind, current_state)
        ON DELETE RESTRICT,
    CONSTRAINT data_rights_retained_scope_kind_check
        CHECK (request_kind = 'deletion'),
    CONSTRAINT data_rights_retained_scope_state_check
        CHECK (completion_state = 'partially_completed'),
    CONSTRAINT data_rights_retained_scope_ref_format_check CHECK (
        retained_scope_ref <> ''
        AND retained_scope_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            retained_scope_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND retained_scope_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    )
);

-- CREATE TABLE IF NOT EXISTS also leaves a same-named older CHECK untouched. Replace that owned
-- definition on every apply so a partial rollout cannot keep accepting identities the domain rejects.
ALTER TABLE data_rights_retained_scope_evidence
    DROP CONSTRAINT IF EXISTS data_rights_retained_scope_ref_format_check;
ALTER TABLE data_rights_retained_scope_evidence
    ADD CONSTRAINT data_rights_retained_scope_ref_format_check CHECK (
        retained_scope_ref <> ''
        AND retained_scope_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            retained_scope_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND retained_scope_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    );

CREATE OR REPLACE FUNCTION reject_data_rights_retained_scope_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'data-rights retained completion scope evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS data_rights_retained_scope_immutable_guard
    ON data_rights_retained_scope_evidence;
CREATE TRIGGER data_rights_retained_scope_immutable_guard
    BEFORE UPDATE OR DELETE ON data_rights_retained_scope_evidence
    FOR EACH ROW
    EXECUTE FUNCTION reject_data_rights_retained_scope_mutation();

DROP TRIGGER IF EXISTS data_rights_retained_scope_truncate_guard
    ON data_rights_retained_scope_evidence;
CREATE TRIGGER data_rights_retained_scope_truncate_guard
    BEFORE TRUNCATE ON data_rights_retained_scope_evidence
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_data_rights_retained_scope_mutation();
