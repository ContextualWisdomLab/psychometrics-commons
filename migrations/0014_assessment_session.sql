CREATE TABLE IF NOT EXISTS assessment_session (
    session_ref TEXT NOT NULL
        CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT NOT NULL
        CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_release_ref TEXT NOT NULL
        CHECK (
            instrument_release_ref = btrim(instrument_release_ref)
            AND instrument_release_ref <> ''
            AND NOT (
                instrument_release_ref ~ '[[:digit:]]'
                AND instrument_release_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_version_ref TEXT NOT NULL
        CHECK (
            instrument_version_ref = btrim(instrument_version_ref)
            AND instrument_version_ref <> ''
            AND NOT (
                instrument_version_ref ~ '[[:digit:]]'
                AND instrument_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_release_content_digest TEXT NOT NULL
        CHECK (instrument_release_content_digest ~ '^sha256:[0-9a-f]{64}$'),
    locale TEXT NOT NULL
        CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    session_state TEXT NOT NULL
        CHECK (
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
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms > 0),
    PRIMARY KEY (session_ref)
);
