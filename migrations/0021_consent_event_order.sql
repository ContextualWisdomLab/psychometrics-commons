DO $$
DECLARE
    event_sequence_column_exists BOOLEAN;
    ambiguous_legacy_history_exists BOOLEAN;
BEGIN
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
            USING ERRCODE = 'check_violation';
    END IF;
END
$$;

ALTER TABLE consent_event
    ADD COLUMN IF NOT EXISTS event_sequence BIGINT;

DO $$
BEGIN
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
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS consent_event_participant_sequence_unique
    ON consent_event (participant_ref, event_sequence)
    WHERE event_sequence IS NOT NULL;
