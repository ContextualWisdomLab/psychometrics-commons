-- Lease references are opaque product references and therefore share the validator installed by
-- 0001_integration_delivery.sql. If integration_reference_is_valid changes in a way that can
-- alter existing-row validity, bump the lease-reference marker below so reapplication rebuilds
-- and revalidates both lease CHECK constraints against the new rule.
ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_worker_ref TEXT;

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_ref TEXT;

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_fencing_token BIGINT;

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_expires_at_unix_ms BIGINT;

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS delivery_lease_generation BIGINT NOT NULL DEFAULT 0;

DO $$
DECLARE
    existing_constraint_oid OID;
    canonical_reference_marker CONSTANT TEXT :=
        'psychometrics-commons:integration-lease-reference:v1';
BEGIN
    SELECT constraint_row.oid
    INTO existing_constraint_oid
    FROM pg_constraint AS constraint_row
    JOIN pg_class AS relation_row
      ON relation_row.oid = constraint_row.conrelid
    JOIN pg_namespace AS namespace_row
      ON namespace_row.oid = relation_row.relnamespace
    WHERE constraint_row.conname = 'integration_outbox_lease_worker_ref_format_check'
      AND relation_row.relname = 'integration_outbox'
      AND namespace_row.nspname = current_schema();

    IF existing_constraint_oid IS NULL
       OR obj_description(existing_constraint_oid, 'pg_constraint')
          IS DISTINCT FROM canonical_reference_marker
    THEN
        IF existing_constraint_oid IS NOT NULL THEN
            ALTER TABLE integration_outbox
                DROP CONSTRAINT integration_outbox_lease_worker_ref_format_check;
        END IF;
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_lease_worker_ref_format_check
            CHECK (
                lease_worker_ref IS NULL
                OR integration_reference_is_valid(lease_worker_ref)
            );
        COMMENT ON CONSTRAINT integration_outbox_lease_worker_ref_format_check
            ON integration_outbox IS 'psychometrics-commons:integration-lease-reference:v1';
    END IF;

    SELECT constraint_row.oid
    INTO existing_constraint_oid
    FROM pg_constraint AS constraint_row
    JOIN pg_class AS relation_row
      ON relation_row.oid = constraint_row.conrelid
    JOIN pg_namespace AS namespace_row
      ON namespace_row.oid = relation_row.relnamespace
    WHERE constraint_row.conname = 'integration_outbox_lease_ref_format_check'
      AND relation_row.relname = 'integration_outbox'
      AND namespace_row.nspname = current_schema();

    IF existing_constraint_oid IS NULL
       OR obj_description(existing_constraint_oid, 'pg_constraint')
          IS DISTINCT FROM canonical_reference_marker
    THEN
        IF existing_constraint_oid IS NOT NULL THEN
            ALTER TABLE integration_outbox
                DROP CONSTRAINT integration_outbox_lease_ref_format_check;
        END IF;
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_lease_ref_format_check
            CHECK (
                lease_ref IS NULL
                OR integration_reference_is_valid(lease_ref)
            );
        COMMENT ON CONSTRAINT integration_outbox_lease_ref_format_check
            ON integration_outbox IS 'psychometrics-commons:integration-lease-reference:v1';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_row
        JOIN pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE constraint_row.conname = 'integration_outbox_lease_fencing_token_positive_check'
          AND relation_row.relname = 'integration_outbox'
          AND namespace_row.nspname = current_schema()
    ) THEN
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_lease_fencing_token_positive_check
            CHECK (lease_fencing_token IS NULL OR lease_fencing_token > 0);
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_row
        JOIN pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE constraint_row.conname = 'integration_outbox_lease_expiry_positive_check'
          AND relation_row.relname = 'integration_outbox'
          AND namespace_row.nspname = current_schema()
    ) THEN
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_lease_expiry_positive_check
            CHECK (lease_expires_at_unix_ms IS NULL OR lease_expires_at_unix_ms > 0);
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_row
        JOIN pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE constraint_row.conname = 'integration_outbox_delivery_lease_generation_nonnegative_check'
          AND relation_row.relname = 'integration_outbox'
          AND namespace_row.nspname = current_schema()
    ) THEN
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_delivery_lease_generation_nonnegative_check
            CHECK (delivery_lease_generation >= 0);
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_row
        JOIN pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE constraint_row.conname = 'integration_outbox_lease_presence_check'
          AND relation_row.relname = 'integration_outbox'
          AND namespace_row.nspname = current_schema()
    ) THEN
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_lease_presence_check
            CHECK (
                (lease_worker_ref IS NULL) = (lease_ref IS NULL)
                AND (lease_worker_ref IS NULL) = (lease_fencing_token IS NULL)
                AND (lease_worker_ref IS NULL) = (lease_expires_at_unix_ms IS NULL)
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_row
        JOIN pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE constraint_row.conname = 'integration_outbox_fencing_generation_check'
          AND relation_row.relname = 'integration_outbox'
          AND namespace_row.nspname = current_schema()
    ) THEN
        ALTER TABLE integration_outbox
            ADD CONSTRAINT integration_outbox_fencing_generation_check
            CHECK (
                lease_fencing_token IS NULL
                OR lease_fencing_token = delivery_lease_generation
            );
    END IF;
END
$$;
