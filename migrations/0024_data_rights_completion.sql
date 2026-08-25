-- Keep completion-evidence and retained-scope reference validation aligned with Rust 1.97's
-- Unicode 17 char::is_numeric contract. PostgreSQL POSIX [[:digit:]] covers decimal digits but
-- does not prove parity for Unicode letter numbers and other numbers such as Roman numerals,
-- superscripts, and vulgar fractions. The generated int4multirange is the exact Unicode 17
-- numeric code-point set used by rustc 1.97. A mixed opaque identifier remains valid because a
-- value is numeric-like only when it contains at least one numeric code point and every character
-- is numeric or one of the numeric spelling tokens accepted by the Rust boundary.
CREATE OR REPLACE FUNCTION data_rights_completion_reference_numeric_like(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $data_rights_completion_numeric$
    WITH reference_character AS (
        SELECT substr(reference_text, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            ascii(character_text) <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange
                AS is_numeric
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
$data_rights_completion_numeric$;

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completion_evidence_ref TEXT;

ALTER TABLE data_rights_request_state
    ADD COLUMN IF NOT EXISTS completed_at_unix_ms BIGINT;

-- Reapplication must replace an earlier revision of this not-yet-released constraint, not merely
-- trust its name. PostgreSQL 18's pg_unicode_fast collation keeps the physical opaque-reference
-- whitespace boundary stable, while the helper above keeps numeric-like classification aligned
-- with the Rust domain boundary across Unicode Nd, Nl, and No categories.
ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completion_evidence_ref_format_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completion_evidence_ref_format_check
    CHECK (
        completion_evidence_ref IS NULL
        OR (
            completion_evidence_ref <> ''
            AND completion_evidence_ref COLLATE "pg_unicode_fast"
                !~ '(^[[:space:]])|([[:space:]]$)'
            AND NOT data_rights_completion_reference_numeric_like(completion_evidence_ref)
        )
    );

-- These owned completion checks are still unreleased. Reapply their exact current definitions so
-- a partially rolled-out schema cannot keep a weaker same-named revision.
ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completed_time_positive_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completed_time_positive_check
    CHECK (completed_at_unix_ms IS NULL OR completed_at_unix_ms > 0);

ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completion_presence_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completion_presence_check
    CHECK ((completion_evidence_ref IS NULL) = (completed_at_unix_ms IS NULL));

ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completion_state_evidence_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completion_state_evidence_check
    CHECK (
        (current_state IN ('completed', 'partially_completed'))
        = (completion_evidence_ref IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
    );

ALTER TABLE data_rights_request_state
    DROP CONSTRAINT IF EXISTS data_rights_completion_after_processing_check;
ALTER TABLE data_rights_request_state
    ADD CONSTRAINT data_rights_completion_after_processing_check
    CHECK (
        completed_at_unix_ms IS NULL
        OR (
            processing_started_at_unix_ms IS NOT NULL
            AND completed_at_unix_ms >= processing_started_at_unix_ms
        )
    );

-- Once the request has crossed into a completion state, its terminal completion row is evidence,
-- not mutable workflow state. The normal processing -> terminal transition is unaffected because
-- this guard evaluates the OLD row. Later direct SQL cannot rewrite or ordinarily delete completion
-- evidence, clocks, identity, or terminal state while still satisfying the CHECK constraints above.
CREATE OR REPLACE FUNCTION reject_data_rights_terminal_completion_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.current_state IN ('completed', 'partially_completed') THEN
        RAISE EXCEPTION 'data-rights terminal completion evidence is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS data_rights_terminal_completion_immutable_guard
    ON data_rights_request_state;
CREATE TRIGGER data_rights_terminal_completion_immutable_guard
    BEFORE UPDATE ON data_rights_request_state
    FOR EACH ROW
    EXECUTE FUNCTION reject_data_rights_terminal_completion_mutation();

-- A completed row without retained-scope children previously had no foreign-key blocker and could
-- be removed by ordinary DELETE even though this migration treats terminal completion as immutable
-- evidence. Keep non-terminal lifecycle cleanup unchanged while failing closed on terminal DELETE.
DROP TRIGGER IF EXISTS data_rights_terminal_completion_delete_guard
    ON data_rights_request_state;
CREATE TRIGGER data_rights_terminal_completion_delete_guard
    BEFORE DELETE ON data_rights_request_state
    FOR EACH ROW
    WHEN (OLD.current_state IN ('completed', 'partially_completed'))
    EXECUTE FUNCTION reject_data_rights_terminal_completion_mutation();

-- The unique key is referenced by the retained-scope foreign key after that table exists, so unlike
-- the CHECK constraints above it is dependency-sensitive and must not be dropped on reapplication.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint AS constraint_record
        JOIN pg_class AS table_record ON table_record.oid = constraint_record.conrelid
        JOIN pg_namespace AS schema_record ON schema_record.oid = table_record.relnamespace
        WHERE constraint_record.conname = 'data_rights_completion_scope_fk_unique'
          AND table_record.relname = 'data_rights_request_state'
          AND schema_record.nspname = current_schema()
    ) THEN
        ALTER TABLE data_rights_request_state
            ADD CONSTRAINT data_rights_completion_scope_fk_unique
            UNIQUE (request_ref, tenant_ref, request_kind, current_state);
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS data_rights_retained_scope_evidence (
    request_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    request_kind TEXT NOT NULL DEFAULT 'deletion',
    completion_state TEXT NOT NULL DEFAULT 'partially_completed',
    retained_scope_ref TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (request_ref, retained_scope_ref),
    CONSTRAINT data_rights_retained_scope_request_fk
        FOREIGN KEY (request_ref, tenant_ref, request_kind, completion_state)
        REFERENCES data_rights_request_state
            (request_ref, tenant_ref, request_kind, current_state)
        ON DELETE RESTRICT,
    CONSTRAINT data_rights_retained_scope_kind_check
        CHECK (request_kind = 'deletion'),
    CONSTRAINT data_rights_retained_scope_state_check
        CHECK (completion_state = 'partially_completed')
);

-- CREATE TABLE IF NOT EXISTS also leaves a same-named older CHECK untouched. Replace that owned
-- definition on every apply so a partial rollout cannot keep accepting identities the domain rejects.
ALTER TABLE data_rights_retained_scope_evidence
    DROP CONSTRAINT IF EXISTS data_rights_retained_scope_ref_format_check;
ALTER TABLE data_rights_retained_scope_evidence
    ADD CONSTRAINT data_rights_retained_scope_ref_format_check CHECK (
        retained_scope_ref <> ''
        AND retained_scope_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT data_rights_completion_reference_numeric_like(retained_scope_ref)
    );

CREATE OR REPLACE FUNCTION reject_data_rights_retained_scope_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'data-rights retained completion scope evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS data_rights_retained_scope_immutable_guard
    ON data_rights_retained_scope_evidence;
CREATE TRIGGER data_rights_retained_scope_immutable_guard
    BEFORE UPDATE OR DELETE ON data_rights_retained_scope_evidence
    FOR EACH ROW
    EXECUTE FUNCTION reject_data_rights_retained_scope_mutation();

DROP TRIGGER IF EXISTS data_rights_retained_scope_truncate_guard
    ON data_rights_retained_scope_evidence;
CREATE TRIGGER data_rights_retained_scope_truncate_guard
    BEFORE TRUNCATE ON data_rights_retained_scope_evidence
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_data_rights_retained_scope_mutation();
