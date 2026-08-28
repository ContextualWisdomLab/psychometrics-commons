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

-- Use the same validator-derived policy marker as the session-header references. The marker hashes
-- PostgreSQL's normalized live function definition, so changing validator semantics automatically
-- invalidates prior validation evidence. Routine reapplication leaves an unchanged constraint and
-- its validation state intact; a changed definition, missing marker, or historical anonymous check
-- triggers one fail-closed rebuild.
DO $assessment_session_command_reference_constraint$
DECLARE
    policy_marker TEXT;
    current_policy_count BIGINT;
    has_historical_constraint BOOLEAN;
BEGIN
    SELECT 'psychometrics-commons:assessment-session-reference:'
           || md5(pg_get_functiondef(
               'assessment_session_reference_is_valid(text)'::regprocedure
           ))
      INTO policy_marker;

    SELECT COUNT(*)
      INTO current_policy_count
      FROM pg_constraint
     WHERE conrelid = 'assessment_session_command'::regclass
       AND conname = 'assessment_session_command_command_ref_format_check'
       AND obj_description(oid, 'pg_constraint') = policy_marker;

    SELECT EXISTS (
        SELECT 1
          FROM pg_constraint
         WHERE conrelid = 'assessment_session_command'::regclass
           AND conname = 'assessment_session_command_command_ref_check'
    ) INTO has_historical_constraint;

    IF current_policy_count <> 1 OR has_historical_constraint THEN
        ALTER TABLE assessment_session_command
            DROP CONSTRAINT IF EXISTS assessment_session_command_command_ref_check;
        ALTER TABLE assessment_session_command
            DROP CONSTRAINT IF EXISTS assessment_session_command_command_ref_format_check;
        ALTER TABLE assessment_session_command
            ADD CONSTRAINT assessment_session_command_command_ref_format_check CHECK (
                assessment_session_reference_is_valid(command_ref)
            );
        EXECUTE format(
            'COMMENT ON CONSTRAINT assessment_session_command_command_ref_format_check ON assessment_session_command IS %L',
            policy_marker
        );
    END IF;
END
$assessment_session_command_reference_constraint$;
