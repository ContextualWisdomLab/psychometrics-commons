-- Durable product-owned anonymous-first participant identity.
--
-- This table stores only the stable Psychometrics Commons participant base record.
-- Optional Keyverse link history remains a separate append-only identity-link concern.
-- PostgreSQL 18's pg_unicode_fast collation gives the reference guards stable Unicode
-- whitespace and decimal-digit classification instead of inheriting host LC_CTYPE behavior.
-- Once inserted, the participant base evidence is immutable. Account-link changes belong in
-- separate append-only history rather than rewriting or deleting the stable participant row.

CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL,

    CONSTRAINT assessment_participant_ref_format_check CHECK (
        participant_ref <> ''
        AND participant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            participant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND participant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
        tenant_ref <> ''
        AND tenant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            tenant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND tenant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT assessment_participant_created_time_positive_check CHECK (
        created_at_unix_ms > 0
    )
);

-- CREATE TABLE IF NOT EXISTS does not reconcile constraints from an earlier revision of this
-- not-yet-released migration. Reapplication therefore replaces the owned checks with the exact
-- current definitions. Existing rows are validated while each stricter constraint is added, so
-- incompatible historical evidence fails the migration rather than remaining silently accepted.
ALTER TABLE assessment_participant
    DROP CONSTRAINT IF EXISTS assessment_participant_ref_format_check;
ALTER TABLE assessment_participant
    ADD CONSTRAINT assessment_participant_ref_format_check CHECK (
        participant_ref <> ''
        AND participant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            participant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND participant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    );

ALTER TABLE assessment_participant
    DROP CONSTRAINT IF EXISTS assessment_participant_tenant_ref_format_check;
ALTER TABLE assessment_participant
    ADD CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
        tenant_ref <> ''
        AND tenant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            tenant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND tenant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    );

ALTER TABLE assessment_participant
    DROP CONSTRAINT IF EXISTS assessment_participant_created_time_positive_check;
ALTER TABLE assessment_participant
    ADD CONSTRAINT assessment_participant_created_time_positive_check CHECK (
        created_at_unix_ms > 0
    );

CREATE OR REPLACE FUNCTION reject_assessment_participant_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'assessment participant base evidence is immutable'
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
