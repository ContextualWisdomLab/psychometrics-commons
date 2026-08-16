ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS operation_ref TEXT;

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS processing_started_at_unix_ms BIGINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_operation_ref_format_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_operation_ref_format_check
            CHECK (
                operation_ref IS NULL
                OR (
                    operation_ref = btrim(operation_ref)
                    AND operation_ref <> ''
                    AND NOT (
                        operation_ref ~ '[[:digit:]]'
                        AND operation_ref ~ '^[[:digit:]+,.eE-]+$'
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
        WHERE constraint_record.conname = 'data_rights_processing_started_time_positive_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_processing_started_time_positive_check
            CHECK (
                processing_started_at_unix_ms IS NULL OR processing_started_at_unix_ms > 0
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
        WHERE constraint_record.conname = 'data_rights_processing_presence_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_processing_presence_check
            CHECK (
                (operation_ref IS NULL) = (processing_started_at_unix_ms IS NULL)
            );
    END IF;
END
$$;
