-- Pin the published instrument-version identity on durable item-delivery
-- ledgers. Release digest is honor-system at construction, so persist must
-- compare version independently. Existing rows stay nullable until they are
-- rematerialized from the published release; new writes always store a valid
-- opaque instrument_version_ref.
ALTER TABLE item_delivery_ledger
    ADD COLUMN IF NOT EXISTS instrument_version_ref TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'item_delivery_ledger_instrument_version_ref_format_check'
          AND table_record.relname = 'item_delivery_ledger'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE item_delivery_ledger
            ADD CONSTRAINT item_delivery_ledger_instrument_version_ref_format_check
            CHECK (
                instrument_version_ref IS NULL
                OR item_delivery_reference_is_valid(instrument_version_ref)
            );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM item_delivery_ledger
        WHERE instrument_version_ref IS NULL
    ) THEN
        ALTER TABLE item_delivery_ledger
            ALTER COLUMN instrument_version_ref SET NOT NULL;
    END IF;
END
$$;
