CREATE TABLE IF NOT EXISTS assessment_session_command (
    session_ref TEXT NOT NULL
        REFERENCES assessment_session (session_ref),
    command_ref TEXT NOT NULL
        CHECK (
            command_ref = btrim(command_ref)
            AND command_ref <> ''
            AND NOT (
                command_ref ~ '[[:digit:]]'
                AND command_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    command_sequence BIGINT NOT NULL CHECK (command_sequence > 0),
    command_name TEXT NOT NULL
        CHECK (
            command_name IN (
                'activate',
                'pause',
                'resume',
                'complete',
                'begin_scoring',
                'record_score',
                'release',
                'expire',
                'cancel',
                'invalidate'
            )
        ),
    resulting_state TEXT NOT NULL
        CHECK (
            resulting_state IN (
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
    PRIMARY KEY (session_ref, command_ref),
    UNIQUE (session_ref, command_sequence)
);
