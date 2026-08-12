CREATE TABLE IF NOT EXISTS integration_outbox (
    event_ref TEXT NOT NULL
        CHECK (
            event_ref = btrim(event_ref)
            AND event_ref <> ''
            AND NOT (event_ref ~ '[[:digit:]]' AND event_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
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
    subject_ref TEXT NOT NULL
        CHECK (
            subject_ref = btrim(subject_ref)
            AND subject_ref <> ''
            AND NOT (subject_ref ~ '[[:digit:]]' AND subject_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    correlation_ref TEXT NOT NULL
        CHECK (
            correlation_ref = btrim(correlation_ref)
            AND correlation_ref <> ''
            AND NOT (
                correlation_ref ~ '[[:digit:]]'
                AND correlation_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    causation_ref TEXT
        CHECK (causation_ref IS NULL OR (
            causation_ref = btrim(causation_ref)
            AND causation_ref <> ''
            AND NOT (
                causation_ref ~ '[[:digit:]]'
                AND causation_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    current_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (current_state IN ('pending', 'delivered', 'quarantined')),
    latest_event_at_unix_ms BIGINT NOT NULL CHECK (latest_event_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (source_ref, tenant_ref, event_ref)
);

CREATE TABLE IF NOT EXISTS integration_delivery_attempt (
    source_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    event_ref TEXT NOT NULL,
    attempt_ref TEXT NOT NULL
        CHECK (
            attempt_ref = btrim(attempt_ref)
            AND attempt_ref <> ''
            AND NOT (attempt_ref ~ '[[:digit:]]' AND attempt_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    delivery_outcome TEXT NOT NULL
        CHECK (delivery_outcome IN ('delivered', 'retryable_failure', 'permanent_failure')),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    cause_code TEXT
        CHECK (cause_code IS NULL OR (
            cause_code = btrim(cause_code)
            AND cause_code <> ''
            AND NOT (cause_code ~ '[[:digit:]]' AND cause_code ~ '^[[:digit:]+,.eE-]+$')
        )),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (source_ref, tenant_ref, event_ref, attempt_ref),
    FOREIGN KEY (source_ref, tenant_ref, event_ref)
        REFERENCES integration_outbox(source_ref, tenant_ref, event_ref)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS integration_inbox (
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
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
    subject_ref TEXT NOT NULL
        CHECK (
            subject_ref = btrim(subject_ref)
            AND subject_ref <> ''
            AND NOT (subject_ref ~ '[[:digit:]]' AND subject_ref ~ '^[[:digit:]+,.eE-]+$')
        ),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    received_at_unix_ms BIGINT NOT NULL CHECK (received_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer_ref, source_ref, tenant_ref, source_event_ref)
);