-- Durable product-owned anonymous-first participant identity.
--
-- This migration requires PostgreSQL major version 18 and a UTF8-encoded database because
-- pg_unicode_fast is a PostgreSQL 18 built-in collation available for UTF8 databases.
-- This table stores only the stable Psychometrics Commons participant base record.
-- Optional Keyverse link history remains a separate append-only identity-link concern.
-- pg_unicode_fast gives the reference guards stable Unicode whitespace and Unicode 16 decimal-digit
-- classification instead of inheriting host LC_CTYPE behavior. Rust's supported standard library
-- uses Unicode 17 for `char::is_numeric`, so the numeric-like helper below adds every Unicode 17
-- Nl/No code point plus the new Unicode 17 Nd code points not present in PostgreSQL 18.
-- Once inserted, the participant base evidence is immutable. Account-link changes belong in
-- separate append-only history rather than rewriting or deleting the stable participant row.

CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL
);

-- Keep opaque-reference validation aligned with Rust `char::is_numeric` (Nd/Nl/No) while the
-- PostgreSQL 18 runtime is pinned to Unicode 16. The explicit multirange comes from Unicode 17.0
-- UnicodeData.txt: all Nl/No ranges plus Unicode 17 Nd ranges absent from Unicode 16. Exact parity
-- and the Rust Unicode-version pin are covered by the real-PostgreSQL regression suite.
CREATE OR REPLACE FUNCTION opaque_reference_numeric_like(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $function$
    WITH reference_character AS (
        SELECT substr(reference_text, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            (
                character_text COLLATE "pg_unicode_fast" ~ '^[[:digit:]]$'
                OR ascii(character_text) <@ '{[178,180),[185,186),[188,191),[2548,2554),[2930,2936),[3056,3059),[3192,3199),[3416,3423),[3440,3449),[3882,3892),[4969,4989),[5870,5873),[6128,6138),[6618,6619),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42726,42736),[43056,43062),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69734),[70113,70133),[71482,71484),[71914,71923),[72794,72813),[73184,73194),[73664,73685),[74752,74863),[93019,93026),[93824,93847),[94196,94199),[119488,119508),[119520,119540),[119648,119673),[125127,125136),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245)}'::int4multirange
            ) AS is_numeric
        FROM reference_character
    )
    SELECT COALESCE(
        bool_or(is_numeric)
        AND bool_and(
            is_numeric
            OR character_text = ANY (
                ARRAY[
                    '+',
                    '-',
                    '.',
                    ',',
                    'e',
                    'E',
                    U&'\066B',
                    U&'\066C',
                    U&'\FF0E',
                    U&'\FF0C'
                ]
            )
        ),
        FALSE
    )
    FROM reference_classification;
$function$;

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
        AND NOT opaque_reference_numeric_like(participant_ref)
    );

ALTER TABLE assessment_participant
    DROP CONSTRAINT IF EXISTS assessment_participant_tenant_ref_format_check;
ALTER TABLE assessment_participant
    ADD CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
        tenant_ref <> ''
        AND tenant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT opaque_reference_numeric_like(tenant_ref)
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
