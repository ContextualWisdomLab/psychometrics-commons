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
