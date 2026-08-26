-- Scoring-request persistence accepts the same opaque-reference shape as the Rust domain.
-- PostgreSQL's POSIX digit class does not include every Unicode character for which Rust 1.97
-- `char::is_numeric` is true. The generated int4multiranges below mirror rustc 1.97 / Unicode 17
-- numeric and Default_Ignorable_Code_Point sets; pg_unicode_fast supplies Unicode
-- whitespace/control classification.
CREATE OR REPLACE FUNCTION scoring_request_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $scoring_request_reference$
    WITH reference_character AS (
        SELECT
            substr(reference_text, character_index, 1) AS character_text,
            ascii(substr(reference_text, character_index, 1)) AS code_point
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
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
$scoring_request_reference$;

CREATE TABLE IF NOT EXISTS scoring_request (
    scoring_request_ref TEXT CONSTRAINT scoring_request_scoring_request_ref_not_null NOT NULL
        CONSTRAINT scoring_request_scoring_request_ref_format_check CHECK (
            scoring_request_reference_is_valid(scoring_request_ref)
        ),
    session_ref TEXT CONSTRAINT scoring_request_session_ref_not_null NOT NULL
        CONSTRAINT scoring_request_session_ref_format_check CHECK (
            scoring_request_reference_is_valid(session_ref)
        ),
    response_snapshot_ref TEXT CONSTRAINT scoring_request_response_snapshot_ref_not_null NOT NULL
        CONSTRAINT scoring_request_response_snapshot_ref_format_check CHECK (
            scoring_request_reference_is_valid(response_snapshot_ref)
        ),
    assessment_spec_ref TEXT CONSTRAINT scoring_request_assessment_spec_ref_not_null NOT NULL
        CONSTRAINT scoring_request_assessment_spec_ref_format_check CHECK (
            scoring_request_reference_is_valid(assessment_spec_ref)
        ),
    instrument_version_ref TEXT CONSTRAINT scoring_request_instrument_version_ref_not_null NOT NULL
        CONSTRAINT scoring_request_instrument_version_ref_format_check CHECK (
            scoring_request_reference_is_valid(instrument_version_ref)
        ),
    scoring_version_ref TEXT CONSTRAINT scoring_request_scoring_version_ref_not_null NOT NULL
        CONSTRAINT scoring_request_scoring_version_ref_format_check CHECK (
            scoring_request_reference_is_valid(scoring_version_ref)
        ),
    calibration_reference TEXT CONSTRAINT scoring_request_calibration_reference_not_null NOT NULL
        CONSTRAINT scoring_request_calibration_reference_format_check CHECK (
            scoring_request_reference_is_valid(calibration_reference)
        ),
    norm_version_ref TEXT
        CONSTRAINT scoring_request_norm_version_ref_format_check CHECK (
            norm_version_ref IS NULL OR scoring_request_reference_is_valid(norm_version_ref)
        ),
    requested_output_schema_version INTEGER
        CONSTRAINT scoring_request_schema_version_not_null NOT NULL
        CONSTRAINT scoring_request_schema_version_positive_check CHECK (
            requested_output_schema_version > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT scoring_request_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT scoring_request_pkey PRIMARY KEY (scoring_request_ref)
);

-- Immutable scoring identity is evidence, not cleanup input. An upgrade must never normalize,
-- rewrite, delete, or silently exempt a historical row that the current Rust boundary rejects.
-- Detect legacy-invalid evidence before touching owned CHECK constraints so an operator can make
-- an explicit, separately governed remediation/quarantine decision with the original row intact.
DO $scoring_request_reference_upgrade$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM scoring_request
        WHERE NOT scoring_request_reference_is_valid(scoring_request_ref)
           OR NOT scoring_request_reference_is_valid(session_ref)
           OR NOT scoring_request_reference_is_valid(response_snapshot_ref)
           OR NOT scoring_request_reference_is_valid(assessment_spec_ref)
           OR NOT scoring_request_reference_is_valid(instrument_version_ref)
           OR NOT scoring_request_reference_is_valid(scoring_version_ref)
           OR NOT scoring_request_reference_is_valid(calibration_reference)
           OR (
               norm_version_ref IS NOT NULL
               AND NOT scoring_request_reference_is_valid(norm_version_ref)
           )
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = 'scoring_request contains legacy reference identities incompatible with the Rust opaque-reference contract',
            CONSTRAINT = 'scoring_request_reference_upgrade_guard';
    END IF;
END
$scoring_request_reference_upgrade$;

-- CREATE TABLE IF NOT EXISTS leaves same-named constraints untouched. Replace the owned reference
-- constraints on every apply so upgrading an existing product schema closes the direct-SQL alias
-- gap instead of fixing only fresh installations. The preflight above guarantees this replacement
-- cannot become an implicit historical-identity rewrite policy.
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_norm_version_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_calibration_reference_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_scoring_version_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_instrument_version_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_assessment_spec_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_response_snapshot_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_session_ref_format_check;
ALTER TABLE scoring_request
    DROP CONSTRAINT IF EXISTS scoring_request_scoring_request_ref_format_check;

ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_scoring_request_ref_format_check CHECK (
        scoring_request_reference_is_valid(scoring_request_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_session_ref_format_check CHECK (
        scoring_request_reference_is_valid(session_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_response_snapshot_ref_format_check CHECK (
        scoring_request_reference_is_valid(response_snapshot_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_assessment_spec_ref_format_check CHECK (
        scoring_request_reference_is_valid(assessment_spec_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_instrument_version_ref_format_check CHECK (
        scoring_request_reference_is_valid(instrument_version_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_scoring_version_ref_format_check CHECK (
        scoring_request_reference_is_valid(scoring_version_ref)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_calibration_reference_format_check CHECK (
        scoring_request_reference_is_valid(calibration_reference)
    );
ALTER TABLE scoring_request
    ADD CONSTRAINT scoring_request_norm_version_ref_format_check CHECK (
        norm_version_ref IS NULL OR scoring_request_reference_is_valid(norm_version_ref)
    );
