DO $$
DECLARE
    event_sequence_column_exists BOOLEAN;
    ambiguous_legacy_history_exists BOOLEAN;
BEGIN
    -- Fence old and new writers while the upgrade proves that every existing
    -- participant history has either no event or exactly one order-unambiguous
    -- legacy event. The whole ordering-schema transition stays in this one
    -- statement so a failed preflight cannot leave a partially upgraded table.
    LOCK TABLE consent_event IN SHARE ROW EXCLUSIVE MODE;

    SELECT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'consent_event'
          AND column_name = 'event_sequence'
    )
    INTO event_sequence_column_exists;

    IF event_sequence_column_exists THEN
        EXECUTE '
            SELECT EXISTS (
                SELECT 1
                FROM consent_event
                WHERE event_sequence IS NULL
                GROUP BY participant_ref
                HAVING COUNT(*) > 1
            )'
        INTO ambiguous_legacy_history_exists;
    ELSE
        SELECT EXISTS (
            SELECT 1
            FROM consent_event
            GROUP BY participant_ref
            HAVING COUNT(*) > 1
        )
        INTO ambiguous_legacy_history_exists;
    END IF;

    IF ambiguous_legacy_history_exists THEN
        RAISE EXCEPTION
            'consent event ordering migration requires deterministic legacy order evidence'
            USING ERRCODE = '23514';
    END IF;

    IF NOT event_sequence_column_exists THEN
        ALTER TABLE consent_event
            ADD COLUMN event_sequence BIGINT;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'consent_event_sequence_positive_check'
          AND conrelid = 'consent_event'::regclass
    ) THEN
        ALTER TABLE consent_event
            ADD CONSTRAINT consent_event_sequence_positive_check
            CHECK (event_sequence IS NULL OR event_sequence > 0);
    END IF;

    EXECUTE '
        CREATE UNIQUE INDEX IF NOT EXISTS consent_event_participant_sequence_unique
            ON consent_event (participant_ref, event_sequence)
            WHERE event_sequence IS NOT NULL';

    -- Keep the one-row legacy compatibility window explicit and fail closed if
    -- an old writer attempts to create a second unsequenced row after upgrade.
    EXECUTE '
        CREATE UNIQUE INDEX IF NOT EXISTS consent_event_participant_legacy_unique
            ON consent_event (participant_ref)
            WHERE event_sequence IS NULL';
END
$$;
