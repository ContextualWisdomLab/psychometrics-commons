ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS verification_evidence_ref TEXT
        CONSTRAINT data_rights_verification_evidence_format_check CHECK (
            verification_evidence_ref IS NULL
            OR data_rights_reference_is_valid(verification_evidence_ref)
        );

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS verified_at_unix_ms BIGINT
        CONSTRAINT data_rights_verified_time_positive_check CHECK (
            verified_at_unix_ms IS NULL OR verified_at_unix_ms > 0
        );

-- ADD COLUMN IF NOT EXISTS does not replace a historical inline CHECK. Recreate the owned
-- evidence-reference constraint so upgrades also validate existing identity-verification rows.
ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_verification_evidence_format_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_verification_evidence_format_check CHECK (
        verification_evidence_ref IS NULL
        OR data_rights_reference_is_valid(verification_evidence_ref)
    );

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_verification_presence_check'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_verification_presence_check
            CHECK (
                (verification_evidence_ref IS NULL) = (verified_at_unix_ms IS NULL)
            );
    END IF;
END
$$;
