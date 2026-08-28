-- Opaque item-delivery references deliberately remain data, not SQL syntax. The
-- persistence adapter binds them as query parameters. These helpers enforce the
-- Rust 1.97 canonical identity boundary: nonblank, no outer Unicode whitespace,
-- no embedded control/default-ignorable characters, and no numeric-like spelling under
-- `char::is_numeric`. PostgreSQL 18 UTF-8 and pg_unicode_fast are runtime prerequisites.
--
-- PostgreSQL assumes functions used by CHECK constraints are immutable. A marker derived from
-- the scalar predicate definition makes semantic upgrades recreate and validate every dependent
-- CHECK once, while ordinary idempotent migration reapplication preserves validated constraints.
CREATE OR REPLACE FUNCTION item_delivery_reference_is_valid(reference_value TEXT)
RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $item_delivery_reference$
    WITH reference_character AS (
        SELECT substr(reference_value, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_value)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            ascii(character_text) <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange AS is_numeric,
            ascii(character_text) <@ '{[173,174),[847,848),[1564,1565),[4447,4449),[6068,6070),[6155,6160),[8203,8208),[8234,8239),[8288,8304),[12644,12645),[65024,65040),[65279,65280),[65440,65441),[65520,65529),[113824,113828),[119155,119163),[917504,921600)}'::int4multirange AS is_default_ignorable
        FROM reference_character
    )
    SELECT
        reference_value IS NOT NULL
        AND reference_value <> ''
        AND reference_value COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND reference_value COLLATE "pg_unicode_fast" !~ '[[:cntrl:]]'
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
$item_delivery_reference$;

DO $item_delivery_array_function$
DECLARE
    migration_schema TEXT := current_schema();
BEGIN
    EXECUTE format(
        $create_item_delivery_array_function$
CREATE OR REPLACE FUNCTION %1$I.item_delivery_reference_array_is_valid(reference_values TEXT[])
RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog, %1$I
AS $item_delivery_reference_array$
    SELECT
        reference_values IS NOT NULL
        AND COUNT(*) = COUNT(DISTINCT reference_value)
        AND COALESCE(
            bool_and(item_delivery_reference_is_valid(reference_value)),
            TRUE
        )
    FROM unnest(reference_values) AS allowed_reference(reference_value);
$item_delivery_reference_array$;
        $create_item_delivery_array_function$,
        migration_schema
    );
END;
$item_delivery_array_function$;

CREATE TABLE IF NOT EXISTS item_delivery_ledger (
    tenant_ref TEXT CONSTRAINT item_delivery_ledger_tenant_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        ),
    session_ref TEXT CONSTRAINT item_delivery_ledger_session_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_session_ref_format_check CHECK (
            item_delivery_reference_is_valid(session_ref)
        ),
    instrument_release_ref TEXT CONSTRAINT item_delivery_ledger_release_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_release_ref_format_check CHECK (
            item_delivery_reference_is_valid(instrument_release_ref)
        ),
    release_content_digest TEXT CONSTRAINT item_delivery_ledger_digest_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_digest_format_check CHECK (
            release_content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    locale TEXT CONSTRAINT item_delivery_ledger_locale_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    allowed_item_version_refs TEXT[] CONSTRAINT item_delivery_ledger_allowed_items_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_allowed_items_not_empty_check CHECK (
            cardinality(allowed_item_version_refs) > 0
        )
        CONSTRAINT item_delivery_ledger_allowed_items_format_check CHECK (
            item_delivery_reference_array_is_valid(allowed_item_version_refs)
        ),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_ledger_pkey PRIMARY KEY (session_ref),
    CONSTRAINT item_delivery_ledger_tenant_session_unique UNIQUE (tenant_ref, session_ref)
);

CREATE TABLE IF NOT EXISTS item_delivery_event (
    tenant_ref TEXT CONSTRAINT item_delivery_event_tenant_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        ),
    session_ref TEXT CONSTRAINT item_delivery_event_session_ref_not_null NOT NULL,
    delivery_event_ref TEXT CONSTRAINT item_delivery_event_delivery_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_delivery_ref_format_check CHECK (
            item_delivery_reference_is_valid(delivery_event_ref)
        ),
    item_version_ref TEXT CONSTRAINT item_delivery_event_item_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_item_ref_format_check CHECK (
            item_delivery_reference_is_valid(item_version_ref)
        ),
    presentation_context_ref TEXT CONSTRAINT item_delivery_event_presentation_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_presentation_ref_format_check CHECK (
            item_delivery_reference_is_valid(presentation_context_ref)
        ),
    selection_evidence_ref TEXT
        CONSTRAINT item_delivery_event_selection_ref_format_check CHECK (
            selection_evidence_ref IS NULL
            OR item_delivery_reference_is_valid(selection_evidence_ref)
        ),
    delivery_sequence BIGINT CONSTRAINT item_delivery_event_sequence_not_null NOT NULL
        CONSTRAINT item_delivery_event_sequence_positive_check CHECK (delivery_sequence > 0),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_event_pkey PRIMARY KEY (session_ref, delivery_event_ref),
    CONSTRAINT item_delivery_event_delivery_ref_unique UNIQUE (delivery_event_ref),
    CONSTRAINT item_delivery_event_session_tenant_fk FOREIGN KEY (tenant_ref, session_ref)
        REFERENCES item_delivery_ledger (tenant_ref, session_ref),
    CONSTRAINT item_delivery_event_item_version_unique UNIQUE (session_ref, item_version_ref),
    CONSTRAINT item_delivery_event_sequence_unique UNIQUE (session_ref, delivery_sequence)
);

DO $item_delivery_reference_constraints$
DECLARE
    reference_contract_version TEXT;
    stored_reference_contract_version TEXT := obj_description(
        'item_delivery_reference_is_valid(text)'::regprocedure,
        'pg_proc'
    );
BEGIN
    SELECT 'psychometrics-commons:item-delivery-reference:'
           || md5(pg_get_functiondef(
               'item_delivery_reference_is_valid(text)'::regprocedure
           ))
      INTO reference_contract_version;

    -- An existing table keeps same-named CHECK definitions untouched, while
    -- CREATE OR REPLACE FUNCTION does not make PostgreSQL scan historical rows after semantic
    -- predicate changes. Recreate predicate-dependent checks only when the validator-derived
    -- marker advances. A fresh install also enters this branch once on empty tables. Replacement
    -- CHECK creation validates all existing rows, so an unsafe historical alias fails the upgrade
    -- atomically; an already-current schema preserves constraint objects and avoids repeated
    -- exclusive locks plus full-table scans on ordinary startup/reapply.
    IF stored_reference_contract_version IS DISTINCT FROM reference_contract_version THEN
        ALTER TABLE item_delivery_event DROP CONSTRAINT IF EXISTS item_delivery_event_selection_ref_format_check;
        ALTER TABLE item_delivery_event DROP CONSTRAINT IF EXISTS item_delivery_event_presentation_ref_format_check;
        ALTER TABLE item_delivery_event DROP CONSTRAINT IF EXISTS item_delivery_event_item_ref_format_check;
        ALTER TABLE item_delivery_event DROP CONSTRAINT IF EXISTS item_delivery_event_delivery_ref_format_check;
        ALTER TABLE item_delivery_event DROP CONSTRAINT IF EXISTS item_delivery_event_tenant_ref_format_check;
        ALTER TABLE item_delivery_ledger DROP CONSTRAINT IF EXISTS item_delivery_ledger_allowed_items_format_check;
        ALTER TABLE item_delivery_ledger DROP CONSTRAINT IF EXISTS item_delivery_ledger_release_ref_format_check;
        ALTER TABLE item_delivery_ledger DROP CONSTRAINT IF EXISTS item_delivery_ledger_session_ref_format_check;
        ALTER TABLE item_delivery_ledger DROP CONSTRAINT IF EXISTS item_delivery_ledger_tenant_ref_format_check;

        ALTER TABLE item_delivery_ledger ADD CONSTRAINT item_delivery_ledger_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        );
        ALTER TABLE item_delivery_ledger ADD CONSTRAINT item_delivery_ledger_session_ref_format_check CHECK (
            item_delivery_reference_is_valid(session_ref)
        );
        ALTER TABLE item_delivery_ledger ADD CONSTRAINT item_delivery_ledger_release_ref_format_check CHECK (
            item_delivery_reference_is_valid(instrument_release_ref)
        );
        ALTER TABLE item_delivery_ledger ADD CONSTRAINT item_delivery_ledger_allowed_items_format_check CHECK (
            item_delivery_reference_array_is_valid(allowed_item_version_refs)
        );
        ALTER TABLE item_delivery_event ADD CONSTRAINT item_delivery_event_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        );
        ALTER TABLE item_delivery_event ADD CONSTRAINT item_delivery_event_delivery_ref_format_check CHECK (
            item_delivery_reference_is_valid(delivery_event_ref)
        );
        ALTER TABLE item_delivery_event ADD CONSTRAINT item_delivery_event_item_ref_format_check CHECK (
            item_delivery_reference_is_valid(item_version_ref)
        );
        ALTER TABLE item_delivery_event ADD CONSTRAINT item_delivery_event_presentation_ref_format_check CHECK (
            item_delivery_reference_is_valid(presentation_context_ref)
        );
        ALTER TABLE item_delivery_event ADD CONSTRAINT item_delivery_event_selection_ref_format_check CHECK (
            selection_evidence_ref IS NULL OR item_delivery_reference_is_valid(selection_evidence_ref)
        );
    END IF;

    EXECUTE format(
        'COMMENT ON FUNCTION item_delivery_reference_is_valid(TEXT) IS %L',
        reference_contract_version
    );
END;
$item_delivery_reference_constraints$;
