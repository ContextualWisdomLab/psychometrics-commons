-- Restricted operational-to-research identity mapping.
-- research_program_ref is stored on both tables as part of the composite
-- foreign key so one operational participant can have only one research
-- identity per program without a non-key transitive dependency.

CREATE TABLE IF NOT EXISTS research_participant (
    research_participant_ref TEXT
        CONSTRAINT research_participant_ref_not_null NOT NULL
        CONSTRAINT research_participant_ref_format_check CHECK (
            research_participant_ref = btrim(research_participant_ref)
            AND research_participant_ref <> ''
            AND NOT (
                research_participant_ref ~ '[[:digit:]]'
                AND research_participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_program_ref TEXT
        CONSTRAINT research_participant_program_ref_not_null NOT NULL
        CONSTRAINT research_participant_program_ref_format_check CHECK (
            research_program_ref = btrim(research_program_ref)
            AND research_program_ref <> ''
            AND research_program_ref <> research_participant_ref
            AND NOT (
                research_program_ref ~ '[[:digit:]]'
                AND research_program_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    recorded_at_unix_ms BIGINT
        CONSTRAINT research_participant_recorded_at_unix_not_null NOT NULL
        CONSTRAINT research_participant_recorded_at_unix_positive_check CHECK (
            recorded_at_unix_ms > 0
        ),
    recorded_at TIMESTAMPTZ
        CONSTRAINT research_participant_recorded_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_participant_pkey PRIMARY KEY (research_participant_ref),
    CONSTRAINT research_participant_program_identity_unique UNIQUE (
        research_participant_ref,
        research_program_ref
    )
);

CREATE TABLE IF NOT EXISTS research_identity_linkage (
    linkage_ref TEXT
        CONSTRAINT research_identity_linkage_ref_not_null NOT NULL
        CONSTRAINT research_identity_linkage_ref_format_check CHECK (
            linkage_ref = btrim(linkage_ref)
            AND linkage_ref <> ''
            AND NOT (
                linkage_ref ~ '[[:digit:]]'
                AND linkage_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT
        CONSTRAINT research_identity_linkage_participant_ref_not_null NOT NULL
        CONSTRAINT research_identity_linkage_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND participant_ref <> linkage_ref
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_participant_ref TEXT
        CONSTRAINT research_identity_linkage_research_ref_not_null NOT NULL
        CONSTRAINT research_identity_linkage_research_ref_format_check CHECK (
            research_participant_ref = btrim(research_participant_ref)
            AND research_participant_ref <> ''
            AND research_participant_ref <> participant_ref
            AND NOT (
                research_participant_ref ~ '[[:digit:]]'
                AND research_participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_program_ref TEXT
        CONSTRAINT research_identity_linkage_program_ref_not_null NOT NULL
        CONSTRAINT research_identity_linkage_program_ref_format_check CHECK (
            research_program_ref = btrim(research_program_ref)
            AND research_program_ref <> ''
            AND research_program_ref <> participant_ref
            AND research_program_ref <> research_participant_ref
            AND NOT (
                research_program_ref ~ '[[:digit:]]'
                AND research_program_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    linkage_key_version TEXT
        CONSTRAINT research_identity_linkage_key_version_not_null NOT NULL
        CONSTRAINT research_identity_linkage_key_version_format_check CHECK (
            linkage_key_version = btrim(linkage_key_version)
            AND linkage_key_version <> ''
            AND NOT (
                linkage_key_version ~ '[[:digit:]]'
                AND linkage_key_version ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    recorded_at_unix_ms BIGINT
        CONSTRAINT research_identity_linkage_recorded_at_unix_not_null NOT NULL
        CONSTRAINT research_identity_linkage_recorded_at_unix_positive_check CHECK (
            recorded_at_unix_ms > 0
        ),
    recorded_at TIMESTAMPTZ
        CONSTRAINT research_identity_linkage_recorded_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_identity_linkage_pkey PRIMARY KEY (linkage_ref),
    CONSTRAINT research_identity_linkage_participant_program_unique UNIQUE (
        participant_ref,
        research_program_ref
    ),
    CONSTRAINT research_identity_linkage_research_participant_unique UNIQUE (
        research_participant_ref
    ),
    CONSTRAINT research_identity_linkage_participant_program_fk FOREIGN KEY (
        research_participant_ref,
        research_program_ref
    ) REFERENCES research_participant (
        research_participant_ref,
        research_program_ref
    )
);

CREATE OR REPLACE VIEW public_research_identity AS
SELECT research_participant_ref, research_program_ref
FROM research_identity_linkage;

CREATE OR REPLACE FUNCTION reject_research_identity_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'restricted research identity evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS research_participant_immutable_guard
    ON research_participant;
CREATE TRIGGER research_participant_immutable_guard
    BEFORE UPDATE OR DELETE ON research_participant
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_identity_mutation();

DROP TRIGGER IF EXISTS research_participant_truncate_guard
    ON research_participant;
CREATE TRIGGER research_participant_truncate_guard
    BEFORE TRUNCATE ON research_participant
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_identity_mutation();

DROP TRIGGER IF EXISTS research_identity_linkage_immutable_guard
    ON research_identity_linkage;
CREATE TRIGGER research_identity_linkage_immutable_guard
    BEFORE UPDATE OR DELETE ON research_identity_linkage
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_identity_mutation();

DROP TRIGGER IF EXISTS research_identity_linkage_truncate_guard
    ON research_identity_linkage;
CREATE TRIGGER research_identity_linkage_truncate_guard
    BEFORE TRUNCATE ON research_identity_linkage
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_identity_mutation();
