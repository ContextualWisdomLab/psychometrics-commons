-- Append-only, purpose-bound product audit evidence.
--
-- Reference validation intentionally mirrors the Rust 1.97 / Unicode 17 opaque-reference
-- boundary used by `src/reference.rs`: blank/padded references, control characters,
-- Default_Ignorable_Code_Point aliases, and numeric-like identities fail closed. The validator is
-- local to this migration so applying migration 0040 in an isolated schema does not depend on a
-- function owned by another persistence aggregate.

CREATE OR REPLACE FUNCTION audit_evidence_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $audit_evidence_reference$
    WITH reference_character AS (
        SELECT
            substr(reference_text, character_index, 1) AS character_text,
            ascii(substr(reference_text, character_index, 1)) AS code_point
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            code_point,
            code_point <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange AS is_numeric,
            code_point <@ '{[173,174),[847,848),[1564,1565),[4447,4449),[6068,6070),[6155,6160),[8203,8208),[8234,8239),[8288,8304),[12644,12645),[65024,65040),[65279,65280),[65440,65441),[65520,65529),[113824,113828),[119155,119163),[917504,921600)}'::int4multirange AS is_default_ignorable
        FROM reference_character
    )
    SELECT
        reference_text <> ''
        AND reference_text COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND reference_text COLLATE "pg_unicode_fast" !~ '[[:cntrl:]]'
        AND NOT COALESCE(bool_or(is_default_ignorable), FALSE)
        AND NOT COALESCE(
            bool_or(is_numeric)
            AND bool_and(
                is_numeric
                OR character_text = ANY (
                    ARRAY['+', '-', '.', ',', 'e', 'E', U&'\066B', U&'\066C', U&'\FF0E', U&'\FF0C']
                )
            ),
            FALSE
        )
    FROM reference_classification;
$audit_evidence_reference$;

DO $audit_evidence_schema$
DECLARE
    relation_ref REGCLASS;
    created_table BOOLEAN;
    actual_columns TEXT[];
    actual_constraint_names TEXT[];
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('psychometrics-commons:migration-0040'));
    relation_ref := to_regclass('audit_evidence_record');
    created_table := relation_ref IS NULL;

    IF created_table THEN
        EXECUTE $create_audit_evidence_record$
CREATE TABLE audit_evidence_record (
    audit_event_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    purpose_code TEXT NOT NULL,
    action_code TEXT NOT NULL,
    resource_ref TEXT NOT NULL,
    outcome_code TEXT NOT NULL,
    evidence_digest TEXT NOT NULL,
    occurred_at_unix_ms BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CONSTRAINT audit_evidence_event_ref_shape_check CHECK (audit_evidence_reference_is_valid(audit_event_ref)),
    CONSTRAINT audit_evidence_tenant_ref_shape_check CHECK (audit_evidence_reference_is_valid(tenant_ref)),
    CONSTRAINT audit_evidence_actor_ref_shape_check CHECK (audit_evidence_reference_is_valid(actor_ref)),
    CONSTRAINT audit_evidence_resource_ref_shape_check CHECK (audit_evidence_reference_is_valid(resource_ref)),
    CONSTRAINT audit_evidence_purpose_code_shape_check CHECK (purpose_code ~ '^[a-z][a-z0-9_]*$'),
    CONSTRAINT audit_evidence_action_code_shape_check CHECK (action_code ~ '^[a-z][a-z0-9_]*$'),
    CONSTRAINT audit_evidence_outcome_allowed_check CHECK (outcome_code IN ('succeeded', 'denied', 'failed')),
    CONSTRAINT audit_evidence_digest_shape_check CHECK (evidence_digest ~ '^sha256:[0-9a-f]{64}$'),
    CONSTRAINT audit_evidence_occurrence_positive_check CHECK (occurred_at_unix_ms > 0)
)
$create_audit_evidence_record$;
        relation_ref := to_regclass('audit_evidence_record');
    END IF;

    IF relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record migration did not create its owned table';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s',
            attribute.attname,
            format_type(attribute.atttypid, attribute.atttypmod),
            CASE WHEN attribute.attnotnull THEN 'not_null' ELSE 'nullable' END
        )
        FROM pg_attribute AS attribute
        WHERE attribute.attrelid = relation_ref
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
        ORDER BY attribute.attnum
    ) INTO actual_columns;

    IF actual_columns IS DISTINCT FROM ARRAY[
        'audit_event_ref:text:not_null',
        'tenant_ref:text:not_null',
        'actor_ref:text:not_null',
        'purpose_code:text:not_null',
        'action_code:text:not_null',
        'resource_ref:text:not_null',
        'outcome_code:text:not_null',
        'evidence_digest:text:not_null',
        'occurred_at_unix_ms:bigint:not_null',
        'recorded_at:timestamp with time zone:not_null'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record column contract does not match migration 0040';
    END IF;

    SELECT ARRAY(
        SELECT constraint_record.conname::TEXT
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraint_names;

    IF actual_constraint_names IS DISTINCT FROM ARRAY[
        'audit_evidence_action_code_shape_check',
        'audit_evidence_actor_ref_shape_check',
        'audit_evidence_digest_shape_check',
        'audit_evidence_event_ref_shape_check',
        'audit_evidence_occurrence_positive_check',
        'audit_evidence_outcome_allowed_check',
        'audit_evidence_purpose_code_shape_check',
        'audit_evidence_record_pkey',
        'audit_evidence_resource_ref_shape_check',
        'audit_evidence_tenant_ref_shape_check'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'audit_evidence_record constraint contract does not match migration 0040';
    END IF;
END
$audit_evidence_schema$;

CREATE INDEX IF NOT EXISTS audit_evidence_tenant_time_index
    ON audit_evidence_record (tenant_ref, occurred_at_unix_ms, audit_event_ref);

CREATE OR REPLACE FUNCTION reject_audit_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION 'audit evidence is append-only'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS audit_evidence_reject_row_mutation ON audit_evidence_record;
CREATE TRIGGER audit_evidence_reject_row_mutation
BEFORE UPDATE OR DELETE ON audit_evidence_record
FOR EACH ROW
EXECUTE FUNCTION reject_audit_evidence_mutation();

DROP TRIGGER IF EXISTS audit_evidence_reject_truncate ON audit_evidence_record;
CREATE TRIGGER audit_evidence_reject_truncate
BEFORE TRUNCATE ON audit_evidence_record
FOR EACH STATEMENT
EXECUTE FUNCTION reject_audit_evidence_mutation();
