ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completion_evidence_ref TEXT;

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completed_at_unix_ms BIGINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_evidence_ref_format_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_evidence_ref_format_check
            CHECK (
                completion_evidence_ref IS NULL
                OR (
                    completion_evidence_ref = btrim(completion_evidence_ref)
                    AND completion_evidence_ref <> ''
                    AND NOT (
                        completion_evidence_ref ~ '[[:digit:]]'
                        AND completion_evidence_ref ~ '^[[:digit:]+,.eE-]+$'
                    )
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

CREATE TABLE IF NOT EXISTS data_rights_retained_scope_evidence (
    request_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    retained_scope_ref TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (request_ref, retained_scope_ref),
    CONSTRAINT data_rights_retained_scope_request_fk
        FOREIGN KEY (request_ref, tenant_ref)
        REFERENCES data_rights_request_state (request_ref, tenant_ref)
        ON DELETE RESTRICT,
    CONSTRAINT data_rights_retained_scope_ref_format_check CHECK (
        retained_scope_ref = btrim(retained_scope_ref)
        AND retained_scope_ref <> ''
        AND NOT (
            retained_scope_ref ~ '[[:digit:]]'
            AND retained_scope_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    )
);
