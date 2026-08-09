CREATE TABLE IF NOT EXISTS integration_outbox (
    event_ref TEXT PRIMARY KEY
        CHECK (event_ref = btrim(event_ref) AND event_ref <> '' AND event_ref !~ '^[0-9]+$'),
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
    source_ref TEXT NOT NULL
        CHECK (source_ref = btrim(source_ref) AND source_ref <> '' AND source_ref !~ '^[0-9]+$'),
    tenant_ref TEXT NOT NULL
        CHECK (tenant_ref = btrim(tenant_ref) AND tenant_ref <> '' AND tenant_ref !~ '^[0-9]+$'),
    subject_ref TEXT NOT NULL
        CHECK (subject_ref = btrim(subject_ref) AND subject_ref <> '' AND subject_ref !~ '^[0-9]+$'),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    correlation_ref TEXT NOT NULL
        CHECK (correlation_ref = btrim(correlation_ref) AND correlation_ref <> '' AND correlation_ref !~ '^[0-9]+$'),
    causation_ref TEXT
        CHECK (causation_ref IS NULL OR (
            causation_ref = btrim(causation_ref)
            AND causation_ref <> ''
            AND causation_ref !~ '^[0-9]+$'
        )),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    current_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (current_state IN ('pending', 'delivered', 'quarantined')),
    latest_event_at_unix_ms BIGINT NOT NULL CHECK (latest_event_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE IF NOT EXISTS integration_delivery_attempt (
    event_ref TEXT NOT NULL REFERENCES integration_outbox(event_ref) ON DELETE CASCADE,
    attempt_ref TEXT NOT NULL
        CHECK (attempt_ref = btrim(attempt_ref) AND attempt_ref <> '' AND attempt_ref !~ '^[0-9]+$'),
    delivery_outcome TEXT NOT NULL
        CHECK (delivery_outcome IN ('delivered', 'retryable_failure', 'permanent_failure')),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    cause_code TEXT
        CHECK (cause_code IS NULL OR (
            cause_code = btrim(cause_code)
            AND cause_code <> ''
            AND cause_code !~ '^[0-9]+$'
        )),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (event_ref, attempt_ref)
);

CREATE TABLE IF NOT EXISTS integration_inbox (
    consumer_ref TEXT NOT NULL
        CHECK (consumer_ref = btrim(consumer_ref) AND consumer_ref <> '' AND consumer_ref !~ '^[0-9]+$'),
    source_ref TEXT NOT NULL
        CHECK (source_ref = btrim(source_ref) AND source_ref <> '' AND source_ref !~ '^[0-9]+$'),
    tenant_ref TEXT NOT NULL
        CHECK (tenant_ref = btrim(tenant_ref) AND tenant_ref <> '' AND tenant_ref !~ '^[0-9]+$'),
    source_event_ref TEXT NOT NULL
        CHECK (source_event_ref = btrim(source_event_ref) AND source_event_ref <> '' AND source_event_ref !~ '^[0-9]+$'),
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
    subject_ref TEXT NOT NULL
        CHECK (subject_ref = btrim(subject_ref) AND subject_ref <> '' AND subject_ref !~ '^[0-9]+$'),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    received_at_unix_ms BIGINT NOT NULL CHECK (received_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer_ref, source_ref, tenant_ref, source_event_ref)
);
