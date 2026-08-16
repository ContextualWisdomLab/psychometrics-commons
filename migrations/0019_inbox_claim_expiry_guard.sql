-- Add a database-authoritative wall-clock deadline to the already-shipped
-- inbox-consumption claim schema without rewriting migration 0012.
--
-- Existing in-flight processing rows predate this server-time evidence. They
-- are conservatively expired at upgrade time so a stale worker cannot gain a
-- fresh lease merely because the migration was installed later.

ALTER TABLE integration_consumption
    ADD COLUMN IF NOT EXISTS claim_deadline_at TIMESTAMPTZ;

UPDATE integration_consumption
SET claim_deadline_at = clock_timestamp()
WHERE consumption_state = 'processing'
  AND claim_deadline_at IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'integration_consumption'::regclass
          AND conname = 'integration_consumption_claim_deadline_shape'
    ) THEN
        ALTER TABLE integration_consumption
            ADD CONSTRAINT integration_consumption_claim_deadline_shape
            CHECK (
                (
                    consumption_state = 'processing'
                    AND claim_expires_at_unix_ms IS NOT NULL
                    AND claim_deadline_at IS NOT NULL
                )
                OR (
                    consumption_state <> 'processing'
                    AND claim_expires_at_unix_ms IS NULL
                    AND claim_deadline_at IS NULL
                )
            ) NOT VALID;
    END IF;
END;
$$;

ALTER TABLE integration_consumption
    VALIDATE CONSTRAINT integration_consumption_claim_deadline_shape;

CREATE OR REPLACE FUNCTION maintain_inbox_claim_deadline()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.consumption_state = 'processing' THEN
        IF OLD.consumption_state <> 'processing' THEN
            NEW.claim_deadline_at := clock_timestamp()
                + ((NEW.claim_expires_at_unix_ms - NEW.latest_event_at_unix_ms)
                    * INTERVAL '1 millisecond');
        ELSE
            NEW.claim_deadline_at := OLD.claim_deadline_at;
        END IF;
    ELSE
        NEW.claim_deadline_at := NULL;
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION reject_expired_inbox_claim_terminal_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.consumption_state = 'processing'
       AND NEW.consumption_state IN ('completed', 'quarantined')
       AND OLD.claim_expires_at_unix_ms IS NOT NULL
       AND OLD.claim_deadline_at IS NOT NULL
       AND (
           NEW.latest_event_at_unix_ms >= OLD.claim_expires_at_unix_ms
           OR clock_timestamp() >= OLD.claim_deadline_at
       ) THEN
        RAISE EXCEPTION 'expired inbox processing claim cannot perform a terminal transition'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS a_integration_consumption_claim_deadline
    ON integration_consumption;
CREATE TRIGGER a_integration_consumption_claim_deadline
    BEFORE UPDATE ON integration_consumption
    FOR EACH ROW
    EXECUTE FUNCTION maintain_inbox_claim_deadline();

DROP TRIGGER IF EXISTS z_integration_consumption_claim_expiry_guard
    ON integration_consumption;
CREATE TRIGGER z_integration_consumption_claim_expiry_guard
    BEFORE UPDATE ON integration_consumption
    FOR EACH ROW
    EXECUTE FUNCTION reject_expired_inbox_claim_terminal_transition();
