-- Inbox-consumption persistence depends on the integration schema and therefore reuses the
-- integration-owned Rust-equivalent opaque-reference predicate installed by migration 0001.
CREATE TABLE IF NOT EXISTS integration_consumption (
    consumer_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_consumer_ref_check CHECK (
            integration_reference_is_valid(consumer_ref)
        ),
    source_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_source_ref_check CHECK (
            integration_reference_is_valid(source_ref)
        ),
    tenant_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_tenant_ref_check CHECK (
            integration_reference_is_valid(tenant_ref)
        ),
    source_event_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_source_event_ref_check CHECK (
            integration_reference_is_valid(source_event_ref)
        ),
    consumption_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_consumption_ref_check CHECK (
            integration_reference_is_valid(consumption_ref)
        ),
    side_effect_ref TEXT NOT NULL
        CONSTRAINT integration_consumption_side_effect_ref_check CHECK (
            integration_reference_is_valid(side_effect_ref)
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
    completion_evidence_ref TEXT
        CONSTRAINT integration_consumption_completion_evidence_ref_check CHECK (
            completion_evidence_ref IS NULL
            OR integration_reference_is_valid(completion_evidence_ref)
        ),
    cause_code TEXT
        CONSTRAINT integration_consumption_cause_code_check CHECK (
            cause_code IS NULL OR integration_reference_is_valid(cause_code)
        ),
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
        )
        OR (
            consumption_state <> 'processing'
            AND claim_expires_at_unix_ms IS NULL
        )
    )
);

-- CREATE TABLE IF NOT EXISTS preserves historical CHECK definitions. Replace every
-- integration-consumption reference CHECK on each apply so an upgrade revalidates existing rows
-- with the exact predicate already used by the outbox/inbox persistence boundary.
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_cause_code_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_completion_evidence_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_side_effect_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_consumption_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_source_event_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_tenant_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_source_ref_check;
ALTER TABLE integration_consumption
    DROP CONSTRAINT IF EXISTS integration_consumption_consumer_ref_check;

ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_consumer_ref_check CHECK (
        integration_reference_is_valid(consumer_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_source_ref_check CHECK (
        integration_reference_is_valid(source_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_tenant_ref_check CHECK (
        integration_reference_is_valid(tenant_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_source_event_ref_check CHECK (
        integration_reference_is_valid(source_event_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_consumption_ref_check CHECK (
        integration_reference_is_valid(consumption_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_side_effect_ref_check CHECK (
        integration_reference_is_valid(side_effect_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_completion_evidence_ref_check CHECK (
        completion_evidence_ref IS NULL
        OR integration_reference_is_valid(completion_evidence_ref)
    );
ALTER TABLE integration_consumption
    ADD CONSTRAINT integration_consumption_cause_code_check CHECK (
        cause_code IS NULL OR integration_reference_is_valid(cause_code)
    );
