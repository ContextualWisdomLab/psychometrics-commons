ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_worker_ref TEXT
        CHECK (
            lease_worker_ref IS NULL
            OR (
                lease_worker_ref = btrim(lease_worker_ref)
                AND lease_worker_ref <> ''
                AND NOT (
                    lease_worker_ref ~ '[[:digit:]]'
                    AND lease_worker_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        );

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_ref TEXT
        CHECK (
            lease_ref IS NULL
            OR (
                lease_ref = btrim(lease_ref)
                AND lease_ref <> ''
                AND NOT (
                    lease_ref ~ '[[:digit:]]'
                    AND lease_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        );

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_fencing_token BIGINT
        CHECK (lease_fencing_token IS NULL OR lease_fencing_token > 0);

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS lease_expires_at_unix_ms BIGINT
        CHECK (lease_expires_at_unix_ms IS NULL OR lease_expires_at_unix_ms > 0);

ALTER TABLE integration_outbox
    ADD COLUMN IF NOT EXISTS delivery_lease_generation BIGINT NOT NULL DEFAULT 0
        CHECK (delivery_lease_generation >= 0);

DO $$
BEGIN
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
