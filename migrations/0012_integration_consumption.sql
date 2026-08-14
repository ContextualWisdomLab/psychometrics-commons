CREATE TABLE IF NOT EXISTS integration_consumption (
    consumer_ref TEXT NOT NULL
        CHECK (
            consumer_ref = btrim(consumer_ref)
            AND consumer_ref <> ''
            AND NOT (consumer_ref ~ '[[:digit:]]' AND consumer_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    source_ref TEXT NOT NULL
        CHECK (
            source_ref = btrim(source_ref)
            AND source_ref <> ''
            AND NOT (source_ref ~ '[[:digit:]]' AND source_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    tenant_ref TEXT NOT NULL
        CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (tenant_ref ~ '[[:digit:]]' AND tenant_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    source_event_ref TEXT NOT NULL
        CHECK (
            source_event_ref = btrim(source_event_ref)
            AND source_event_ref <> ''
            AND NOT (
                source_event_ref ~ '[[:digit:]]'
                AND source_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consumption_ref TEXT NOT NULL
        CHECK (
            consumption_ref = btrim(consumption_ref)
            AND consumption_ref <> ''
            AND NOT (
                consumption_ref ~ '[[:digit:]]'
                AND consumption_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    side_effect_ref TEXT NOT NULL
        CHECK (
            side_effect_ref = btrim(side_effect_ref)
            AND side_effect_ref <> ''
            AND NOT (
                side_effect_ref ~ '[[:digit:]]'
                AND side_effect_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consumption_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (consumption_state IN ('pending', 'processing', 'completed', 'quarantined')),
    fencing_token BIGINT NOT NULL DEFAULT 0
        CHECK (fencing_token >= 0),
    latest_event_at_unix_ms BIGINT NOT NULL
        CHECK (latest_event_at_unix_ms > 0),
    claim_expires_at_unix_ms BIGINT
        CHECK (
            claim_expires_at_unix_ms IS NULL
            OR claim_expires_at_unix_ms > latest_event_at_unix_ms
        ),
    claim_deadline_at TIMESTAMPTZ,
    completion_evidence_ref TEXT
        CHECK (completion_evidence_ref IS NULL OR (
            completion_evidence_ref = btrim(completion_evidence_ref)
            AND completion_evidence_ref <> ''
            AND NOT (
                completion_evidence_ref ~ '[[:digit:]]'
                AND completion_evidence_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    cause_code TEXT
        CHECK (cause_code IS NULL OR (
            cause_code = btrim(cause_code)
            AND cause_code <> ''
            AND NOT (cause_code ~ '[[:digit:]]' AND cause_code ~ '^[[:digit:]+,.eE-]+$')
        )),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (
        consumer_ref, source_ref, tenant_ref, source_event_ref, consumption_ref
    ),
    UNIQUE (consumer_ref, source_ref, tenant_ref, source_event_ref, side_effect_ref),
    FOREIGN KEY (consumer_ref, source_ref, tenant_ref, source_event_ref)
        REFERENCES integration_inbox (
            consumer_ref, source_ref, tenant_ref, source_event_ref
        )
        ON DELETE RESTRICT,
    CHECK (
        (
            consumption_state IN ('pending', 'processing')
            AND completion_evidence_ref IS NULL
            AND cause_code IS NULL
        )
        OR (
            consumption_state = 'completed'
            AND completion_evidence_ref IS NOT NULL
            AND cause_code IS NULL
        )
        OR (
            consumption_state = 'quarantined'
            AND cause_code IS NOT NULL
            AND completion_evidence_ref IS NULL
        )
    ),
    CHECK (
        (
            consumption_state = 'processing'
            AND fencing_token > 0
            AND claim_expires_at_unix_ms IS NOT NULL
            AND claim_deadline_at IS NOT NULL
        )
        OR (
            consumption_state <> 'processing'
            AND claim_expires_at_unix_ms IS NULL
            AND claim_deadline_at IS NULL
        )
    )
);

CREATE OR REPLACE FUNCTION maintain_inbox_claim_deadline()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.consumption_state = 'processing'
       AND OLD.consumption_state <> 'processing' THEN
        NEW.claim_deadline_at := clock_timestamp()
            + ((NEW.claim_expires_at_unix_ms - NEW.latest_event_at_unix_ms)
                * INTERVAL '1 millisecond');
    ELSIF NEW.consumption_state <> 'processing' THEN
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
