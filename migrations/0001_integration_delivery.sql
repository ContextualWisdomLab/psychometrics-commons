-- Integration persistence accepts exactly the same public opaque-reference shape as the Rust
-- domain. PostgreSQL's POSIX digit class does not include every Unicode character for which Rust
-- 1.97 `char::is_numeric` is true. The generated int4multirange below is rustc 1.97's Unicode 17
-- numeric set; pg_unicode_fast supplies stable Unicode whitespace/control classification.
CREATE OR REPLACE FUNCTION integration_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $integration_reference$
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
                        '+', '-', '.', ',', 'e', 'E',
                        U&'\066B', U&'\066C', U&'\FF0E', U&'\FF0C'
                    ]
                )
            ),
            FALSE
        )
    FROM reference_classification;
$integration_reference$;

CREATE TABLE IF NOT EXISTS integration_outbox (
    event_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(event_ref)),
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
    source_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(source_ref)),
    tenant_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(tenant_ref)),
    subject_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(subject_ref)),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    correlation_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(correlation_ref)),
    causation_ref TEXT
        CHECK (causation_ref IS NULL OR integration_reference_is_valid(causation_ref)),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    current_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (current_state IN ('pending', 'delivered', 'quarantined')),
    latest_event_at_unix_ms BIGINT NOT NULL CHECK (latest_event_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (source_ref, tenant_ref, event_ref)
);

CREATE TABLE IF NOT EXISTS integration_delivery_attempt (
    source_ref TEXT NOT NULL,
    tenant_ref TEXT NOT NULL,
    event_ref TEXT NOT NULL,
    attempt_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(attempt_ref)),
    delivery_outcome TEXT NOT NULL
        CHECK (delivery_outcome IN ('delivered', 'retryable_failure', 'permanent_failure')),
    occurred_at_unix_ms BIGINT NOT NULL CHECK (occurred_at_unix_ms > 0),
    cause_code TEXT
        CHECK (cause_code IS NULL OR integration_reference_is_valid(cause_code)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (source_ref, tenant_ref, event_ref, attempt_ref),
    FOREIGN KEY (source_ref, tenant_ref, event_ref)
        REFERENCES integration_outbox(source_ref, tenant_ref, event_ref)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS integration_inbox (
    consumer_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(consumer_ref)),
    source_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(source_ref)),
    tenant_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(tenant_ref)),
    source_event_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(source_event_ref)),
    event_type TEXT NOT NULL
        CHECK (event_type = btrim(event_type) AND event_type <> '' AND octet_length(event_type) <= 128),
    schema_version TEXT NOT NULL
        CHECK (schema_version = btrim(schema_version) AND schema_version <> '' AND octet_length(schema_version) <= 64),
    subject_ref TEXT NOT NULL
        CHECK (integration_reference_is_valid(subject_ref)),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    received_at_unix_ms BIGINT NOT NULL CHECK (received_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (consumer_ref, source_ref, tenant_ref, source_event_ref)
);

-- CREATE TABLE IF NOT EXISTS leaves same-named historical CHECK constraints untouched. The repair
-- marker is derived from PostgreSQL's installed validator definition, so CREATE OR REPLACE FUNCTION
-- above automatically changes the marker whenever validator semantics or properties change. A
-- newly created or historically weakened CHECK is rebuilt and tagged, while unchanged definitions
-- keep the canonical constraint object stable on later reapplications.
DO $integration_reference_constraints$
DECLARE
    constraint_spec RECORD;
    existing_constraint_oid OID;
    canonical_marker CONSTANT TEXT :=
        'psychometrics-commons:integration-reference:'
        || pg_catalog.md5(
            pg_catalog.pg_get_functiondef(
                pg_catalog.to_regprocedure(
                    pg_catalog.format(
                        '%I.integration_reference_is_valid(text)',
                        pg_catalog.current_schema()
                    )
                )
            )
        );
BEGIN
    FOR constraint_spec IN
        SELECT *
        FROM (VALUES
            ('integration_outbox', 'integration_outbox_event_ref_check',
             'CHECK (integration_reference_is_valid(event_ref))'),
            ('integration_outbox', 'integration_outbox_source_ref_check',
             'CHECK (integration_reference_is_valid(source_ref))'),
            ('integration_outbox', 'integration_outbox_tenant_ref_check',
             'CHECK (integration_reference_is_valid(tenant_ref))'),
            ('integration_outbox', 'integration_outbox_subject_ref_check',
             'CHECK (integration_reference_is_valid(subject_ref))'),
            ('integration_outbox', 'integration_outbox_correlation_ref_check',
             'CHECK (integration_reference_is_valid(correlation_ref))'),
            ('integration_outbox', 'integration_outbox_causation_ref_check',
             'CHECK (causation_ref IS NULL OR integration_reference_is_valid(causation_ref))'),
            ('integration_delivery_attempt', 'integration_delivery_attempt_attempt_ref_check',
             'CHECK (integration_reference_is_valid(attempt_ref))'),
            ('integration_delivery_attempt', 'integration_delivery_attempt_cause_code_check',
             'CHECK (cause_code IS NULL OR integration_reference_is_valid(cause_code))'),
            ('integration_inbox', 'integration_inbox_consumer_ref_check',
             'CHECK (integration_reference_is_valid(consumer_ref))'),
            ('integration_inbox', 'integration_inbox_source_ref_check',
             'CHECK (integration_reference_is_valid(source_ref))'),
            ('integration_inbox', 'integration_inbox_tenant_ref_check',
             'CHECK (integration_reference_is_valid(tenant_ref))'),
            ('integration_inbox', 'integration_inbox_source_event_ref_check',
             'CHECK (integration_reference_is_valid(source_event_ref))'),
            ('integration_inbox', 'integration_inbox_subject_ref_check',
             'CHECK (integration_reference_is_valid(subject_ref))')
        ) AS owned_constraint(relation_name, constraint_name, constraint_definition)
    LOOP
        SELECT constraint_row.oid
        INTO existing_constraint_oid
        FROM pg_catalog.pg_constraint AS constraint_row
        JOIN pg_catalog.pg_class AS relation_row
          ON relation_row.oid = constraint_row.conrelid
        JOIN pg_catalog.pg_namespace AS namespace_row
          ON namespace_row.oid = relation_row.relnamespace
        WHERE namespace_row.nspname = current_schema()
          AND relation_row.relname = constraint_spec.relation_name
          AND constraint_row.conname = constraint_spec.constraint_name;

        IF existing_constraint_oid IS NULL
           OR pg_catalog.obj_description(existing_constraint_oid, 'pg_constraint')
              IS DISTINCT FROM canonical_marker
        THEN
            IF existing_constraint_oid IS NOT NULL THEN
                EXECUTE format(
                    'ALTER TABLE %I DROP CONSTRAINT %I',
                    constraint_spec.relation_name,
                    constraint_spec.constraint_name
                );
            END IF;

            EXECUTE format(
                'ALTER TABLE %I ADD CONSTRAINT %I %s',
                constraint_spec.relation_name,
                constraint_spec.constraint_name,
                constraint_spec.constraint_definition
            );
            EXECUTE format(
                'COMMENT ON CONSTRAINT %I ON %I IS %L',
                constraint_spec.constraint_name,
                constraint_spec.relation_name,
                canonical_marker
            );
        END IF;
    END LOOP;
END
$integration_reference_constraints$;
