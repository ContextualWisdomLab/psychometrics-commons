-- Assessment-session persistence accepts the same opaque-reference shape as the Rust domain.
-- PostgreSQL's POSIX digit class does not include every Unicode character for which Rust 1.97
-- `char::is_numeric` is true. The generated int4multirange below is rustc 1.97's Unicode 17
-- numeric set, while pg_unicode_fast supplies Unicode whitespace/control classification.
CREATE OR REPLACE FUNCTION assessment_session_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $assessment_session_reference$
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
    SELECT
        reference_text <> ''
        AND reference_text COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND reference_text COLLATE "pg_unicode_fast" !~ '[[:cntrl:]]'
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
$assessment_session_reference$;

CREATE TABLE IF NOT EXISTS assessment_session (
    session_ref TEXT NOT NULL
        CONSTRAINT assessment_session_session_ref_format_check CHECK (
            assessment_session_reference_is_valid(session_ref)
        ),
    participant_ref TEXT NOT NULL
        CONSTRAINT assessment_session_participant_ref_format_check CHECK (
            assessment_session_reference_is_valid(participant_ref)
        ),
    instrument_release_ref TEXT NOT NULL
        CONSTRAINT assessment_session_release_ref_format_check CHECK (
            assessment_session_reference_is_valid(instrument_release_ref)
        ),
    instrument_version_ref TEXT NOT NULL
        CONSTRAINT assessment_session_version_ref_format_check CHECK (
            assessment_session_reference_is_valid(instrument_version_ref)
        ),
    instrument_release_content_digest TEXT NOT NULL
        CHECK (instrument_release_content_digest ~ '^sha256:[0-9a-f]{64}$'),
    locale TEXT NOT NULL
        CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    session_state TEXT NOT NULL
        CHECK (
            session_state IN (
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
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms > 0),
    PRIMARY KEY (session_ref)
);

-- Older schemas used anonymous CHECK constraints generated from the column names. Remove those
-- historical predicates when present, then recreate stable named constraints on every apply so
-- upgrades scan existing rows under the Rust-equivalent identity contract.
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_session_ref_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_participant_ref_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_instrument_release_ref_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_instrument_version_ref_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_version_ref_format_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_release_ref_format_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_participant_ref_format_check;
ALTER TABLE assessment_session DROP CONSTRAINT IF EXISTS assessment_session_session_ref_format_check;

ALTER TABLE assessment_session ADD CONSTRAINT assessment_session_session_ref_format_check CHECK (
    assessment_session_reference_is_valid(session_ref)
);
ALTER TABLE assessment_session ADD CONSTRAINT assessment_session_participant_ref_format_check CHECK (
    assessment_session_reference_is_valid(participant_ref)
);
ALTER TABLE assessment_session ADD CONSTRAINT assessment_session_release_ref_format_check CHECK (
    assessment_session_reference_is_valid(instrument_release_ref)
);
ALTER TABLE assessment_session ADD CONSTRAINT assessment_session_version_ref_format_check CHECK (
    assessment_session_reference_is_valid(instrument_version_ref)
);
