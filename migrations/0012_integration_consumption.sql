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
        (consumption_state = 'processing' AND fencing_token > 0)
        OR (consumption_state <> 'processing')
    )
);
