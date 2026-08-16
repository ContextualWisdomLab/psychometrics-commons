CREATE TABLE IF NOT EXISTS assessment_session (
    session_ref TEXT CONSTRAINT assessment_session_session_ref_not_null NOT NULL
        CONSTRAINT assessment_session_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT assessment_session_participant_ref_not_null NOT NULL
        CONSTRAINT assessment_session_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_release_ref TEXT CONSTRAINT assessment_session_release_ref_not_null NOT NULL
        CONSTRAINT assessment_session_release_ref_format_check CHECK (
            instrument_release_ref = btrim(instrument_release_ref)
            AND instrument_release_ref <> ''
            AND NOT (
                instrument_release_ref ~ '[[:digit:]]'
                AND instrument_release_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_version_ref TEXT CONSTRAINT assessment_session_version_ref_not_null NOT NULL
        CONSTRAINT assessment_session_version_ref_format_check CHECK (
            instrument_version_ref = btrim(instrument_version_ref)
            AND instrument_version_ref <> ''
            AND NOT (
                instrument_version_ref ~ '[[:digit:]]'
                AND instrument_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_release_content_digest TEXT
        CONSTRAINT assessment_session_digest_not_null NOT NULL
        CONSTRAINT assessment_session_digest_format_check CHECK (
            instrument_release_content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    locale TEXT CONSTRAINT assessment_session_locale_not_null NOT NULL
        CONSTRAINT assessment_session_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    session_state TEXT CONSTRAINT assessment_session_state_not_null NOT NULL
        CONSTRAINT assessment_session_state_values_check CHECK (
            session_state IN (
                'created',
                'active',
                'paused',
                'completed',
                'scoring',
                'scored',
                'released',
                'expired',
                'cancelled',
                'invalidated'
            )
        ),
    created_at_unix_ms BIGINT CONSTRAINT assessment_session_created_at_not_null NOT NULL
        CONSTRAINT assessment_session_created_at_positive_check CHECK (created_at_unix_ms > 0),
    CONSTRAINT assessment_session_pkey PRIMARY KEY (session_ref)
);
