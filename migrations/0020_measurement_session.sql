CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT NOT NULL
        CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT NOT NULL
        CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms > 0),
    PRIMARY KEY (participant_ref)
);

CREATE TABLE IF NOT EXISTS measurement_session (
    session_ref TEXT NOT NULL
        CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT NOT NULL
        CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    owner_participant_ref TEXT NOT NULL
        REFERENCES assessment_participant (participant_ref),
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms > 0),
    PRIMARY KEY (session_ref)
);

CREATE TABLE IF NOT EXISTS session_membership (
    session_ref TEXT NOT NULL
        REFERENCES measurement_session (session_ref),
    participant_ref TEXT NOT NULL
        REFERENCES assessment_participant (participant_ref),
    enrolled_at_unix_ms BIGINT NOT NULL CHECK (enrolled_at_unix_ms > 0),
    PRIMARY KEY (session_ref, participant_ref)
);

CREATE TABLE IF NOT EXISTS session_consent_record (
    session_ref TEXT NOT NULL
        REFERENCES measurement_session (session_ref),
    event_ref TEXT NOT NULL
        CHECK (
            event_ref = btrim(event_ref)
            AND event_ref <> ''
            AND NOT (
                event_ref ~ '[[:digit:]]'
                AND event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT NOT NULL
        REFERENCES assessment_participant (participant_ref),
    encryption_nonce BYTEA NOT NULL
        CHECK (octet_length(encryption_nonce) = 12),
    ciphertext_payload BYTEA NOT NULL
        CHECK (octet_length(ciphertext_payload) > 16),
    PRIMARY KEY (session_ref, event_ref)
);

CREATE TABLE IF NOT EXISTS session_audit_event (
    session_ref TEXT NOT NULL
        REFERENCES measurement_session (session_ref),
    event_ref TEXT NOT NULL
        CHECK (
            event_ref = btrim(event_ref)
            AND event_ref <> ''
            AND NOT (
                event_ref ~ '[[:digit:]]'
                AND event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    actor_ref TEXT NOT NULL
        CHECK (
            actor_ref = btrim(actor_ref)
            AND actor_ref <> ''
            AND NOT (
                actor_ref ~ '[[:digit:]]'
                AND actor_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    encryption_nonce BYTEA NOT NULL
        CHECK (octet_length(encryption_nonce) = 12),
    ciphertext_payload BYTEA NOT NULL
        CHECK (octet_length(ciphertext_payload) > 16),
    PRIMARY KEY (session_ref, event_ref)
);

CREATE TABLE IF NOT EXISTS export_snapshot_pointer (
    session_ref TEXT NOT NULL
        REFERENCES measurement_session (session_ref),
    snapshot_ref TEXT NOT NULL
        CHECK (
            snapshot_ref = btrim(snapshot_ref)
            AND snapshot_ref <> ''
            AND NOT (
                snapshot_ref ~ '[[:digit:]]'
                AND snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    request_ref TEXT NOT NULL
        CHECK (
            request_ref = btrim(request_ref)
            AND request_ref <> ''
            AND NOT (
                request_ref ~ '[[:digit:]]'
                AND request_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    content_digest TEXT NOT NULL
        CHECK (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms > 0),
    PRIMARY KEY (session_ref)
);
