CREATE TABLE IF NOT EXISTS response_snapshot (
    snapshot_ref TEXT CONSTRAINT response_snapshot_snapshot_ref_not_null NOT NULL
        CONSTRAINT response_snapshot_snapshot_ref_format_check CHECK (
            snapshot_ref = btrim(snapshot_ref)
            AND snapshot_ref <> ''
            AND NOT (
                snapshot_ref ~ '[[:digit:]]'
                AND snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT response_snapshot_session_ref_not_null NOT NULL
        CONSTRAINT response_snapshot_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    event_count BIGINT CONSTRAINT response_snapshot_event_count_not_null NOT NULL
        CONSTRAINT response_snapshot_event_count_nonnegative_check CHECK (event_count >= 0),
    last_sequence BIGINT
        CONSTRAINT response_snapshot_last_sequence_positive_check CHECK (
            last_sequence IS NULL OR last_sequence > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT response_snapshot_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT response_snapshot_pkey PRIMARY KEY (snapshot_ref),
    CONSTRAINT response_snapshot_session_unique UNIQUE (session_ref)
);

CREATE TABLE IF NOT EXISTS response_snapshot_entry (
    snapshot_ref TEXT CONSTRAINT response_snapshot_entry_snapshot_ref_not_null NOT NULL,
    snapshot_sequence BIGINT CONSTRAINT response_snapshot_entry_sequence_not_null NOT NULL
        CONSTRAINT response_snapshot_entry_sequence_positive_check CHECK (snapshot_sequence > 0),
    event_ref TEXT CONSTRAINT response_snapshot_entry_event_ref_not_null NOT NULL
        CONSTRAINT response_snapshot_entry_event_ref_format_check CHECK (
            event_ref = btrim(event_ref)
            AND event_ref <> ''
            AND NOT (
                event_ref ~ '[[:digit:]]'
                AND event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    item_version_ref TEXT CONSTRAINT response_snapshot_entry_item_version_ref_not_null NOT NULL
        CONSTRAINT response_snapshot_entry_item_version_ref_format_check CHECK (
            item_version_ref = btrim(item_version_ref)
            AND item_version_ref <> ''
            AND NOT (
                item_version_ref ~ '[[:digit:]]'
                AND item_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    payload_digest TEXT CONSTRAINT response_snapshot_entry_payload_digest_not_null NOT NULL
        CONSTRAINT response_snapshot_entry_payload_digest_not_blank_check CHECK (
            payload_digest = btrim(payload_digest)
            AND payload_digest <> ''
        ),
    created_at TIMESTAMPTZ CONSTRAINT response_snapshot_entry_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT response_snapshot_entry_pkey PRIMARY KEY (snapshot_ref, snapshot_sequence),
    CONSTRAINT response_snapshot_entry_snapshot_fk FOREIGN KEY (snapshot_ref)
        REFERENCES response_snapshot (snapshot_ref),
    CONSTRAINT response_snapshot_entry_event_unique UNIQUE (snapshot_ref, event_ref)
);
