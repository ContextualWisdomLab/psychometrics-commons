-- Response-event persistence accepts exactly the same public opaque-reference shape as the Rust
-- domain. PostgreSQL's POSIX digit class does not include every Unicode character for which
-- Rust 1.97 `char::is_numeric` is true. The generated int4multirange below is rustc 1.97's
-- Unicode 17 numeric set; pg_unicode_fast supplies Unicode whitespace/control classification.
CREATE OR REPLACE FUNCTION response_event_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $response_event_reference$
    WITH reference_character AS (
        SELECT substr(reference_text, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            ascii(character_text) <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange AS is_numeric
        FROM reference_character
    )
    SELECT
        reference_text <> ''
        AND reference_text COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND reference_text COLLATE "pg_unicode_fast" !~ '[[:cntrl:]]'
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
$response_event_reference$;

DO $response_event_schema$
DECLARE
    relation_ref REGCLASS := to_regclass('response_event');
    created_table BOOLEAN := relation_ref IS NULL;
    expected_relation_ref REGCLASS;
    actual_columns TEXT[];
    expected_columns TEXT[];
    actual_defaults TEXT[];
    expected_defaults TEXT[];
    actual_constraints TEXT[];
    expected_constraints TEXT[];
BEGIN
    IF created_table THEN
        EXECUTE $create_response_event$
CREATE TABLE response_event (
    response_event_ref TEXT CONSTRAINT response_event_response_event_ref_not_null NOT NULL
        CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_reference_is_valid(response_event_ref)
        ),
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL
        CONSTRAINT response_event_session_ref_format_check CHECK (
            response_event_reference_is_valid(session_ref)
        ),
    client_event_ref TEXT CONSTRAINT response_event_client_event_ref_not_null NOT NULL
        CONSTRAINT response_event_client_event_ref_format_check CHECK (
            response_event_reference_is_valid(client_event_ref)
        ),
    item_version_ref TEXT CONSTRAINT response_event_item_version_ref_not_null NOT NULL
        CONSTRAINT response_event_item_version_ref_format_check CHECK (
            response_event_reference_is_valid(item_version_ref)
        ),
    payload_digest TEXT CONSTRAINT response_event_payload_digest_not_null NOT NULL
        CONSTRAINT response_event_payload_digest_format_check CHECK (
            payload_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    observed_at TIMESTAMPTZ CONSTRAINT response_event_observed_at_not_null NOT NULL,
    received_at TIMESTAMPTZ CONSTRAINT response_event_received_at_not_null NOT NULL,
    CONSTRAINT response_event_pkey PRIMARY KEY (response_event_ref),
    CONSTRAINT response_event_session_client_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_session_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_observed_not_after_received_check CHECK (observed_at <= received_at)
)
$create_response_event$;
        relation_ref := to_regclass('response_event');
    END IF;

    IF relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event migration did not create its owned table';
    END IF;

    -- Reapplication must repair a weakened same-named reference CHECK rather than treating its
    -- presence as evidence. Adding the replacement constraints validates all historical rows and
    -- therefore fails closed if an earlier schema admitted an identity Rust cannot reconstruct.
    ALTER TABLE response_event
        DROP CONSTRAINT IF EXISTS response_event_item_version_ref_format_check;
    ALTER TABLE response_event
        DROP CONSTRAINT IF EXISTS response_event_client_event_ref_format_check;
    ALTER TABLE response_event
        DROP CONSTRAINT IF EXISTS response_event_session_ref_format_check;
    ALTER TABLE response_event
        DROP CONSTRAINT IF EXISTS response_event_response_event_ref_format_check;

    ALTER TABLE response_event
        ADD CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_reference_is_valid(response_event_ref)
        );
    ALTER TABLE response_event
        ADD CONSTRAINT response_event_session_ref_format_check CHECK (
            response_event_reference_is_valid(session_ref)
        );
    ALTER TABLE response_event
        ADD CONSTRAINT response_event_client_event_ref_format_check CHECK (
            response_event_reference_is_valid(client_event_ref)
        );
    ALTER TABLE response_event
        ADD CONSTRAINT response_event_item_version_ref_format_check CHECK (
            response_event_reference_is_valid(item_version_ref)
        );

    EXECUTE $create_expected_response_event$
CREATE TEMP TABLE expected_response_event_contract (
    response_event_ref TEXT CONSTRAINT response_event_response_event_ref_not_null NOT NULL
        CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_reference_is_valid(response_event_ref)
        ),
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL
        CONSTRAINT response_event_session_ref_format_check CHECK (
            response_event_reference_is_valid(session_ref)
        ),
    client_event_ref TEXT CONSTRAINT response_event_client_event_ref_not_null NOT NULL
        CONSTRAINT response_event_client_event_ref_format_check CHECK (
            response_event_reference_is_valid(client_event_ref)
        ),
    item_version_ref TEXT CONSTRAINT response_event_item_version_ref_not_null NOT NULL
        CONSTRAINT response_event_item_version_ref_format_check CHECK (
            response_event_reference_is_valid(item_version_ref)
        ),
    payload_digest TEXT CONSTRAINT response_event_payload_digest_not_null NOT NULL
        CONSTRAINT response_event_payload_digest_format_check CHECK (
            payload_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    observed_at TIMESTAMPTZ CONSTRAINT response_event_observed_at_not_null NOT NULL,
    received_at TIMESTAMPTZ CONSTRAINT response_event_received_at_not_null NOT NULL,
    CONSTRAINT response_event_pkey PRIMARY KEY (response_event_ref),
    CONSTRAINT response_event_session_client_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_session_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_observed_not_after_received_check CHECK (observed_at <= received_at)
) ON COMMIT DROP
$create_expected_response_event$;
    expected_relation_ref := to_regclass('pg_temp.expected_response_event_contract');

    IF expected_relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event migration could not construct its schema contract';
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

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s',
            attribute.attname,
            format_type(attribute.atttypid, attribute.atttypmod),
            CASE WHEN attribute.attnotnull THEN 'not_null' ELSE 'nullable' END
        )
        FROM pg_attribute AS attribute
        WHERE attribute.attrelid = expected_relation_ref
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
        ORDER BY attribute.attnum
    ) INTO expected_columns;

    IF actual_columns IS DISTINCT FROM expected_columns THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event column contract does not match migration 0020';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            attribute.attname,
            pg_get_expr(default_value.adbin, default_value.adrelid)
        )
        FROM pg_attribute AS attribute
        JOIN pg_attrdef AS default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        WHERE attribute.attrelid = relation_ref
        ORDER BY attribute.attnum
    ) INTO actual_defaults;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            attribute.attname,
            pg_get_expr(default_value.adbin, default_value.adrelid)
        )
        FROM pg_attribute AS attribute
        JOIN pg_attrdef AS default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        WHERE attribute.attrelid = expected_relation_ref
        ORDER BY attribute.attnum
    ) INTO expected_defaults;

    IF actual_defaults IS DISTINCT FROM expected_defaults THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event default contract does not match migration 0020';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s:%s:%s',
            constraint_record.conname,
            constraint_record.contype,
            constraint_record.convalidated,
            constraint_record.conenforced,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraints;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s:%s:%s',
            constraint_record.conname,
            constraint_record.contype,
            constraint_record.convalidated,
            constraint_record.conenforced,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = expected_relation_ref
          AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO expected_constraints;

    IF actual_constraints IS DISTINCT FROM expected_constraints THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event constraint contract does not match migration 0020';
    END IF;
END
$response_event_schema$;
