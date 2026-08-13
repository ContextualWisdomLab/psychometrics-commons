CREATE TABLE IF NOT EXISTS item_delivery_ledger (
    session_ref TEXT CONSTRAINT item_delivery_ledger_session_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_release_ref TEXT CONSTRAINT item_delivery_ledger_release_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_release_ref_format_check CHECK (
            instrument_release_ref = btrim(instrument_release_ref)
            AND instrument_release_ref <> ''
            AND NOT (
                instrument_release_ref ~ '[[:digit:]]'
                AND instrument_release_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    release_content_digest TEXT CONSTRAINT item_delivery_ledger_digest_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_digest_format_check CHECK (
            release_content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    locale TEXT CONSTRAINT item_delivery_ledger_locale_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    allowed_item_version_refs TEXT[] CONSTRAINT item_delivery_ledger_allowed_items_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_allowed_items_not_empty_check CHECK (
            cardinality(allowed_item_version_refs) > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_ledger_pkey PRIMARY KEY (session_ref)
);

CREATE TABLE IF NOT EXISTS item_delivery_event (
    session_ref TEXT CONSTRAINT item_delivery_event_session_ref_not_null NOT NULL,
    delivery_event_ref TEXT CONSTRAINT item_delivery_event_delivery_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_delivery_ref_format_check CHECK (
            delivery_event_ref = btrim(delivery_event_ref)
            AND delivery_event_ref <> ''
            AND NOT (
                delivery_event_ref ~ '[[:digit:]]'
                AND delivery_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    item_version_ref TEXT CONSTRAINT item_delivery_event_item_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_item_ref_format_check CHECK (
            item_version_ref = btrim(item_version_ref)
            AND item_version_ref <> ''
            AND NOT (
                item_version_ref ~ '[[:digit:]]'
                AND item_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    presentation_context_ref TEXT CONSTRAINT item_delivery_event_presentation_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_presentation_ref_format_check CHECK (
            presentation_context_ref = btrim(presentation_context_ref)
            AND presentation_context_ref <> ''
            AND NOT (
                presentation_context_ref ~ '[[:digit:]]'
                AND presentation_context_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    selection_evidence_ref TEXT
        CONSTRAINT item_delivery_event_selection_ref_format_check CHECK (
            selection_evidence_ref IS NULL OR (
                selection_evidence_ref = btrim(selection_evidence_ref)
                AND selection_evidence_ref <> ''
                AND NOT (
                    selection_evidence_ref ~ '[[:digit:]]'
                    AND selection_evidence_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    delivery_sequence BIGINT CONSTRAINT item_delivery_event_sequence_not_null NOT NULL
        CONSTRAINT item_delivery_event_sequence_positive_check CHECK (delivery_sequence > 0),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_event_pkey PRIMARY KEY (session_ref, delivery_event_ref),
    CONSTRAINT item_delivery_event_session_fk FOREIGN KEY (session_ref)
        REFERENCES item_delivery_ledger (session_ref),
    CONSTRAINT item_delivery_event_item_version_unique UNIQUE (session_ref, item_version_ref),
    CONSTRAINT item_delivery_event_sequence_unique UNIQUE (session_ref, delivery_sequence)
);
