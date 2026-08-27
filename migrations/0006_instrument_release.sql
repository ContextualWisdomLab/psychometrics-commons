-- Instrument-release persistence accepts the same opaque-reference shape as the Rust domain.
-- PostgreSQL's POSIX digit class does not include every Unicode character for which Rust 1.97
-- `char::is_numeric` is true. The generated int4multiranges below mirror rustc 1.97 / Unicode 17
-- numeric and Default_Ignorable_Code_Point sets; pg_unicode_fast supplies Unicode
-- whitespace/control classification.
CREATE OR REPLACE FUNCTION instrument_release_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $instrument_release_reference$
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
$instrument_release_reference$;

-- Array-valued provenance owns canonical element validation and exact uniqueness. Cardinality is
-- deliberately left to the pre-existing *_not_empty_check constraints so each invariant has one
-- diagnostic owner and corruption tests can disable the complete relevant boundary explicitly.
DO $instrument_release_array_function$
DECLARE
    migration_schema TEXT := current_schema();
BEGIN
    EXECUTE format(
        $create_instrument_release_array_function$
CREATE OR REPLACE FUNCTION %1$I.instrument_release_reference_array_is_valid(reference_values TEXT[])
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog, %1$I
AS $instrument_release_reference_array$
    SELECT
        COUNT(*) = COUNT(DISTINCT reference_value)
        AND COALESCE(
            bool_and(instrument_release_reference_is_valid(reference_value)),
            TRUE
        )
    FROM unnest(reference_values) AS release_reference(reference_value);
$instrument_release_reference_array$;
        $create_instrument_release_array_function$,
        migration_schema
    );
END;
$instrument_release_array_function$;

CREATE TABLE IF NOT EXISTS instrument_release (
    release_ref TEXT CONSTRAINT instrument_release_release_ref_not_null NOT NULL
        CONSTRAINT instrument_release_release_ref_format_check CHECK (
            instrument_release_reference_is_valid(release_ref)
        ),
    instrument_ref TEXT CONSTRAINT instrument_release_instrument_ref_not_null NOT NULL
        CONSTRAINT instrument_release_instrument_ref_format_check CHECK (
            instrument_release_reference_is_valid(instrument_ref)
        ),
    instrument_version_ref TEXT CONSTRAINT instrument_release_version_ref_not_null NOT NULL
        CONSTRAINT instrument_release_version_ref_format_check CHECK (
            instrument_release_reference_is_valid(instrument_version_ref)
        ),
    construct_ref TEXT CONSTRAINT instrument_release_construct_ref_not_null NOT NULL
        CONSTRAINT instrument_release_construct_ref_format_check CHECK (
            instrument_release_reference_is_valid(construct_ref)
        ),
    item_version_refs TEXT[] CONSTRAINT instrument_release_item_refs_not_null NOT NULL
        CONSTRAINT instrument_release_item_refs_not_empty_check CHECK (
            cardinality(item_version_refs) > 0
        )
        CONSTRAINT instrument_release_item_refs_format_check CHECK (
            instrument_release_reference_array_is_valid(item_version_refs)
        ),
    locale TEXT CONSTRAINT instrument_release_locale_not_null NOT NULL
        CONSTRAINT instrument_release_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    assessment_spec_ref TEXT CONSTRAINT instrument_release_spec_ref_not_null NOT NULL
        CONSTRAINT instrument_release_spec_ref_format_check CHECK (
            instrument_release_reference_is_valid(assessment_spec_ref)
        ),
    scoring_version_ref TEXT CONSTRAINT instrument_release_scoring_ref_not_null NOT NULL
        CONSTRAINT instrument_release_scoring_ref_format_check CHECK (
            instrument_release_reference_is_valid(scoring_version_ref)
        ),
    calibration_reference TEXT CONSTRAINT instrument_release_calibration_ref_not_null NOT NULL
        CONSTRAINT instrument_release_calibration_ref_format_check CHECK (
            instrument_release_reference_is_valid(calibration_reference)
        ),
    norm_version_ref TEXT
        CONSTRAINT instrument_release_norm_ref_format_check CHECK (
            norm_version_ref IS NULL OR instrument_release_reference_is_valid(norm_version_ref)
        ),
    narrative_version_ref TEXT CONSTRAINT instrument_release_narrative_ref_not_null NOT NULL
        CONSTRAINT instrument_release_narrative_ref_format_check CHECK (
            instrument_release_reference_is_valid(narrative_version_ref)
        ),
    consent_requirement_refs TEXT[] CONSTRAINT instrument_release_consent_refs_not_null NOT NULL
        CONSTRAINT instrument_release_consent_refs_not_empty_check CHECK (
            cardinality(consent_requirement_refs) > 0
        )
        CONSTRAINT instrument_release_consent_refs_format_check CHECK (
            instrument_release_reference_array_is_valid(consent_requirement_refs)
        ),
    intended_use_ref TEXT CONSTRAINT instrument_release_intended_use_ref_not_null NOT NULL
        CONSTRAINT instrument_release_intended_use_ref_format_check CHECK (
            instrument_release_reference_is_valid(intended_use_ref)
        ),
    limitations_ref TEXT CONSTRAINT instrument_release_limitations_ref_not_null NOT NULL
        CONSTRAINT instrument_release_limitations_ref_format_check CHECK (
            instrument_release_reference_is_valid(limitations_ref)
        ),
    content_digest TEXT CONSTRAINT instrument_release_digest_not_null NOT NULL
        CONSTRAINT instrument_release_digest_format_check CHECK (
            content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    publication_state TEXT CONSTRAINT instrument_release_state_not_null NOT NULL
        CONSTRAINT instrument_release_state_value_check CHECK (
            publication_state IN ('draft', 'review', 'published', 'suspended', 'retired')
        ),
    created_at_unix_ms BIGINT CONSTRAINT instrument_release_created_at_unix_not_null NOT NULL
        CONSTRAINT instrument_release_created_at_unix_positive_check CHECK (created_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT instrument_release_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT instrument_release_pkey PRIMARY KEY (release_ref)
);

-- CREATE TABLE IF NOT EXISTS leaves same-named constraints untouched. Replace every owned
-- reference constraint whenever the migration is applied so upgrades revalidate historical rows
-- under the exact Rust-equivalent predicate rather than fixing only new installations.
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_consent_refs_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_item_refs_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_limitations_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_intended_use_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_narrative_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_norm_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_calibration_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_scoring_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_spec_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_construct_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_version_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_instrument_ref_format_check;
ALTER TABLE instrument_release DROP CONSTRAINT IF EXISTS instrument_release_release_ref_format_check;

ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_release_ref_format_check CHECK (
    instrument_release_reference_is_valid(release_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_instrument_ref_format_check CHECK (
    instrument_release_reference_is_valid(instrument_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_version_ref_format_check CHECK (
    instrument_release_reference_is_valid(instrument_version_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_construct_ref_format_check CHECK (
    instrument_release_reference_is_valid(construct_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_item_refs_format_check CHECK (
    instrument_release_reference_array_is_valid(item_version_refs)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_spec_ref_format_check CHECK (
    instrument_release_reference_is_valid(assessment_spec_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_scoring_ref_format_check CHECK (
    instrument_release_reference_is_valid(scoring_version_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_calibration_ref_format_check CHECK (
    instrument_release_reference_is_valid(calibration_reference)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_norm_ref_format_check CHECK (
    norm_version_ref IS NULL OR instrument_release_reference_is_valid(norm_version_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_narrative_ref_format_check CHECK (
    instrument_release_reference_is_valid(narrative_version_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_consent_refs_format_check CHECK (
    instrument_release_reference_array_is_valid(consent_requirement_refs)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_intended_use_ref_format_check CHECK (
    instrument_release_reference_is_valid(intended_use_ref)
);
ALTER TABLE instrument_release ADD CONSTRAINT instrument_release_limitations_ref_format_check CHECK (
    instrument_release_reference_is_valid(limitations_ref)
);