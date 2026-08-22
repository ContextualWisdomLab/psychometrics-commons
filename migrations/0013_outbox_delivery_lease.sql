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

-- Lease identities are product-owned opaque references too. Drop the historical definitions on
-- every apply so upgrades revalidate them through migration 0001's Rust-equivalent predicate.
ALTER TABLE integration_outbox
    DROP CONSTRAINT IF EXISTS integration_outbox_lease_worker_ref_format_check;
ALTER TABLE integration_outbox
    DROP CONSTRAINT IF EXISTS integration_outbox_lease_ref_format_check;
ALTER TABLE integration_outbox
    ADD CONSTRAINT integration_outbox_lease_worker_ref_format_check
    CHECK (
        lease_worker_ref IS NULL
        OR integration_reference_is_valid(lease_worker_ref)
    );
ALTER TABLE integration_outbox
    ADD CONSTRAINT integration_outbox_lease_ref_format_check
    CHECK (
        lease_ref IS NULL
        OR integration_reference_is_valid(lease_ref)
    );

DO $$
BEGIN
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
