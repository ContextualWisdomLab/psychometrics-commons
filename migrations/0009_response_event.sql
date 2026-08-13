CREATE TABLE IF NOT EXISTS response_event_ledger (
    session_ref TEXT CONSTRAINT response_event_ledger_session_ref_not_null NOT NULL
        CONSTRAINT response_event_ledger_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at TIMESTAMPTZ CONSTRAINT response_event_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT response_event_ledger_pkey PRIMARY KEY (session_ref)
);

CREATE TABLE IF NOT EXISTS response_event (
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL,
    server_event_ref TEXT CONSTRAINT response_event_server_event_ref_not_null NOT NULL
        CONSTRAINT response_event_server_event_ref_format_check CHECK (
            server_event_ref = btrim(server_event_ref)
            AND server_event_ref <> ''
            AND NOT (
                server_event_ref ~ '[[:digit:]]'
                AND server_event_ref ~ '^[[:digit:]+,.eE-]+$'
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
        CONSTRAINT response_event_payload_digest_not_empty_check CHECK (
            payload_digest = btrim(payload_digest)
            AND payload_digest <> ''
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    created_at TIMESTAMPTZ CONSTRAINT response_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT response_event_pkey PRIMARY KEY (session_ref, server_event_ref),
    CONSTRAINT response_event_client_event_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_server_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_ledger_fkey
        FOREIGN KEY (session_ref) REFERENCES response_event_ledger (session_ref)
);
