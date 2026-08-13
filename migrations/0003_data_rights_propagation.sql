-- Durable participant data-rights request and propagation evidence.
CREATE TABLE IF NOT EXISTS data_rights_request_state (
    request_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    participant_ref TEXT NOT NULL,
    request_kind TEXT NOT NULL,
    scope_ref TEXT NOT NULL,
    current_state TEXT NOT NULL,
    requested_at_unix_ms BIGINT NOT NULL,
    latest_event_at_unix_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT data_rights_request_ref_format_check CHECK (
        request_ref = btrim(request_ref)
        AND request_ref <> ''
        AND NOT (
            request_ref ~ '[[:digit:]]'
            AND request_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_tenant_ref_format_check CHECK (
        tenant_ref = btrim(tenant_ref)
        AND tenant_ref <> ''
        AND NOT (
            tenant_ref ~ '[[:digit:]]'
            AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_participant_ref_format_check CHECK (
        participant_ref = btrim(participant_ref)
        AND participant_ref <> ''
        AND NOT (
            participant_ref ~ '[[:digit:]]'
            AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_scope_ref_format_check CHECK (
        scope_ref = btrim(scope_ref)
        AND scope_ref <> ''
        AND NOT (
            scope_ref ~ '[[:digit:]]'
            AND scope_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_request_kind_valid CHECK (request_kind IN ('export', 'deletion')),
    CONSTRAINT data_rights_request_state_valid CHECK (current_state IN ('requested','identity_verified','processing','completed','partially_completed','rejected','failed')),
    CONSTRAINT data_rights_request_time_positive CHECK (requested_at_unix_ms > 0),
    CONSTRAINT data_rights_latest_time_monotonic CHECK (latest_event_at_unix_ms >= requested_at_unix_ms),
    CONSTRAINT data_rights_request_tenant_unique UNIQUE (request_ref, tenant_ref)
);

CREATE TABLE IF NOT EXISTS data_rights_propagation_state (
    request_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    dependent_system_ref TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    event_ref TEXT NOT NULL,
    current_state TEXT NOT NULL DEFAULT 'pending',
    latest_event_at_unix_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (request_ref, dependent_system_ref),
    CONSTRAINT data_rights_propagation_request_fk FOREIGN KEY (request_ref, tenant_ref) REFERENCES data_rights_request_state (request_ref, tenant_ref) ON DELETE RESTRICT,
    CONSTRAINT data_rights_propagation_outbox_fk FOREIGN KEY (source_ref, tenant_ref, event_ref) REFERENCES integration_outbox (source_ref, tenant_ref, event_ref) ON DELETE RESTRICT,
    CONSTRAINT data_rights_dependent_system_ref_format_check CHECK (
        dependent_system_ref = btrim(dependent_system_ref)
        AND dependent_system_ref <> ''
        AND NOT (
            dependent_system_ref ~ '[[:digit:]]'
            AND dependent_system_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_propagation_source_ref_format_check CHECK (
        source_ref = btrim(source_ref)
        AND source_ref <> ''
        AND NOT (
            source_ref ~ '[[:digit:]]'
            AND source_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_propagation_event_ref_format_check CHECK (
        event_ref = btrim(event_ref)
        AND event_ref <> ''
        AND NOT (
            event_ref ~ '[[:digit:]]'
            AND event_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT data_rights_propagation_state_valid CHECK (current_state IN ('pending','delivered','quarantined')),
    CONSTRAINT data_rights_propagation_time_positive CHECK (latest_event_at_unix_ms > 0),
    CONSTRAINT data_rights_propagation_event_unique UNIQUE (source_ref, tenant_ref, event_ref)
);

CREATE INDEX IF NOT EXISTS data_rights_request_participant_idx ON data_rights_request_state (tenant_ref, participant_ref, requested_at_unix_ms);
CREATE INDEX IF NOT EXISTS data_rights_propagation_state_idx ON data_rights_propagation_state (current_state, latest_event_at_unix_ms);
