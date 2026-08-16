-- Opaque response-event references remain data, not SQL syntax. The persistence
-- adapter binds them as query parameters. These checks enforce the canonical
-- identity boundary (nonblank, nonnumeric-like, no outer whitespace).
CREATE TABLE IF NOT EXISTS response_event (
    response_event_ref TEXT CONSTRAINT response_event_response_event_ref_not_null NOT NULL
        CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_ref = btrim(response_event_ref)
            AND response_event_ref <> ''
            AND NOT (
                response_event_ref ~ '[[:digit:]]'
                AND response_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL
        CONSTRAINT response_event_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    client_event_ref TEXT CONSTRAINT response_event_client_event_ref_not_null NOT NULL
        CONSTRAINT response_event_client_event_ref_format_check CHECK (
            client_event_ref = btrim(client_event_ref)
            AND client_event_ref <> ''
            AND NOT (
                client_event_ref ~ '[[:digit:]]'
                AND client_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    item_version_ref TEXT CONSTRAINT response_event_item_version_ref_not_null NOT NULL
        CONSTRAINT response_event_item_version_ref_format_check CHECK (
            item_version_ref = btrim(item_version_ref)
            AND item_version_ref <> ''
            AND NOT (
                item_version_ref ~ '[[:digit:]]'
                AND item_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    payload_digest TEXT CONSTRAINT response_event_payload_digest_not_null NOT NULL
        CONSTRAINT response_event_payload_digest_format_check CHECK (
            payload_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    observed_at TIMESTAMPTZ CONSTRAINT response_event_observed_at_not_null NOT NULL,
    received_at TIMESTAMPTZ CONSTRAINT response_event_received_at_not_null NOT NULL,
    CONSTRAINT response_event_pkey PRIMARY KEY (response_event_ref),
    CONSTRAINT response_event_session_client_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_session_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_observed_not_after_received_check CHECK (observed_at <= received_at)
);
