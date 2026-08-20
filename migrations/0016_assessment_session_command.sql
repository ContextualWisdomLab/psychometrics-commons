CREATE TABLE IF NOT EXISTS assessment_session_command (
    session_ref TEXT NOT NULL
        REFERENCES assessment_session (session_ref),
    command_ref TEXT NOT NULL
        CONSTRAINT assessment_session_command_command_ref_format_check CHECK (
            assessment_session_reference_is_valid(command_ref)
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

-- The original migration used an anonymous command_ref CHECK. Replace that historical predicate
-- when present and recreate the named Rust-equivalent constraint on every apply so command-history
-- identity cannot retain an alias the application boundary would reject.
ALTER TABLE assessment_session_command
    DROP CONSTRAINT IF EXISTS assessment_session_command_command_ref_check;
ALTER TABLE assessment_session_command
    DROP CONSTRAINT IF EXISTS assessment_session_command_command_ref_format_check;
ALTER TABLE assessment_session_command
    ADD CONSTRAINT assessment_session_command_command_ref_format_check CHECK (
        assessment_session_reference_is_valid(command_ref)
    );
