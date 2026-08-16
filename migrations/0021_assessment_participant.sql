CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT CONSTRAINT assessment_participant_participant_ref_not_null NOT NULL
        CONSTRAINT assessment_participant_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT CONSTRAINT assessment_participant_tenant_ref_not_null NOT NULL
        CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_status TEXT CONSTRAINT assessment_participant_status_not_null NOT NULL
        CONSTRAINT assessment_participant_status_value_check CHECK (
            participant_status = 'anonymous'
        ),
    created_at_unix_ms BIGINT CONSTRAINT assessment_participant_created_at_unix_not_null NOT NULL
        CONSTRAINT assessment_participant_created_at_unix_positive_check CHECK (
            created_at_unix_ms > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT assessment_participant_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT assessment_participant_pkey PRIMARY KEY (participant_ref)
);

CREATE OR REPLACE FUNCTION reject_assessment_participant_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'assessment participant evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS assessment_participant_immutable_guard
    ON assessment_participant;
CREATE TRIGGER assessment_participant_immutable_guard
    BEFORE UPDATE OR DELETE ON assessment_participant
    FOR EACH ROW
    EXECUTE FUNCTION reject_assessment_participant_mutation();

DROP TRIGGER IF EXISTS assessment_participant_truncate_guard
    ON assessment_participant;
CREATE TRIGGER assessment_participant_truncate_guard
    BEFORE TRUNCATE ON assessment_participant
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_assessment_participant_mutation();
