-- Scoring-job persistence accepts exactly the same opaque-reference shape as the Rust domain.
-- PostgreSQL's POSIX digit class does not include every Unicode character for which Rust 1.97
-- `char::is_numeric` is true. The generated int4multirange below is rustc 1.97's Unicode 17
-- numeric set, while pg_unicode_fast supplies Unicode whitespace/control classification.
CREATE OR REPLACE FUNCTION scoring_job_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $scoring_job_reference$
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
$scoring_job_reference$;

DO $scoring_job_schema$
DECLARE
    relation_ref REGCLASS := to_regclass('scoring_job_state');
    created_table BOOLEAN := relation_ref IS NULL;
    actual_columns TEXT[];
    actual_defaults TEXT[];
    actual_constraint_names TEXT[];
    actual_constraints TEXT[];
    actual_constraint_manifest TEXT;
    stored_constraint_manifest TEXT;
    current_reference_contract BOOLEAN := FALSE;
    legacy_reference_contract BOOLEAN := FALSE;
    constraint_manifest_prefix CONSTANT TEXT :=
        'psychometrics-commons:migration-0002:constraint-manifest:';
    probe_job_ref TEXT := 'scoring_job_migration_probe_' || pg_backend_pid()::TEXT;
    probe_request_ref TEXT := 'scoring_request_migration_probe_' || pg_backend_pid()::TEXT;
BEGIN
    IF created_table THEN
        EXECUTE $create_scoring_job_state$
CREATE TABLE scoring_job_state (
    scoring_job_ref TEXT CONSTRAINT scoring_job_ref_not_null NOT NULL
        CONSTRAINT scoring_job_ref_format_check CHECK (
            scoring_job_reference_is_valid(scoring_job_ref)
        ),
    scoring_request_ref TEXT CONSTRAINT scoring_request_ref_not_null NOT NULL
        CONSTRAINT scoring_request_ref_format_check CHECK (
            scoring_job_reference_is_valid(scoring_request_ref)
        ),
    scoring_state TEXT CONSTRAINT scoring_state_not_null NOT NULL
        CONSTRAINT scoring_state_value_check CHECK (
            scoring_state IN (
                'queued',
                'leased',
                'retry_scheduled',
                'completed',
                'quarantined',
                'cancelled'
            )
        ),
    attempt_count INTEGER CONSTRAINT scoring_attempt_count_not_null NOT NULL DEFAULT 0
        CONSTRAINT scoring_attempt_count_nonnegative_check CHECK (attempt_count >= 0),
    max_attempts INTEGER CONSTRAINT scoring_max_attempts_not_null NOT NULL
        CONSTRAINT scoring_max_attempts_positive_check CHECK (max_attempts > 0),
    next_attempt_at_unix_ms BIGINT
        CONSTRAINT scoring_next_attempt_positive_check CHECK (next_attempt_at_unix_ms > 0),
    last_failure_code TEXT
        CONSTRAINT scoring_failure_code_format_check CHECK (
            last_failure_code IS NULL
            OR scoring_job_reference_is_valid(last_failure_code)
        ),
    active_worker_ref TEXT
        CONSTRAINT scoring_worker_ref_format_check CHECK (
            active_worker_ref IS NULL
            OR scoring_job_reference_is_valid(active_worker_ref)
        ),
    active_lease_ref TEXT
        CONSTRAINT scoring_lease_ref_format_check CHECK (
            active_lease_ref IS NULL
            OR scoring_job_reference_is_valid(active_lease_ref)
        ),
    active_fencing_token BIGINT,
    active_lease_expires_at_unix_ms BIGINT,
    result_ref TEXT,
    completed_fencing_token BIGINT,
    created_at TIMESTAMPTZ CONSTRAINT scoring_created_at_not_null NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ CONSTRAINT scoring_updated_at_not_null NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT scoring_job_state_pkey PRIMARY KEY (scoring_job_ref),
    CONSTRAINT scoring_attempt_budget_check CHECK (attempt_count <= max_attempts),
    CONSTRAINT scoring_active_lease_shape_check CHECK (
        (scoring_state = 'leased'
            AND active_worker_ref IS NOT NULL
            AND active_lease_ref IS NOT NULL
            AND active_fencing_token IS NOT NULL
            AND active_lease_expires_at_unix_ms IS NOT NULL)
        OR
        (scoring_state <> 'leased'
            AND active_worker_ref IS NULL
            AND active_lease_ref IS NULL
            AND active_fencing_token IS NULL
            AND active_lease_expires_at_unix_ms IS NULL)
    ),
    CONSTRAINT scoring_fencing_token_positive_check CHECK (
        active_fencing_token IS NULL OR active_fencing_token > 0
    ),
    CONSTRAINT scoring_fencing_attempt_match_check CHECK (
        active_fencing_token IS NULL OR active_fencing_token = attempt_count
    ),
    CONSTRAINT scoring_lease_expiry_positive_check CHECK (
        active_lease_expires_at_unix_ms IS NULL OR active_lease_expires_at_unix_ms > 0
    ),
    CONSTRAINT scoring_completed_fence_positive_check CHECK (
        completed_fencing_token IS NULL OR completed_fencing_token > 0
    ),
    CONSTRAINT scoring_result_ref_format_check CHECK (
        result_ref IS NULL OR scoring_job_reference_is_valid(result_ref)
    ),
    CONSTRAINT scoring_state_shape_check CHECK (
        (scoring_state = 'queued'
            AND attempt_count = 0
            AND next_attempt_at_unix_ms IS NULL
            AND last_failure_code IS NULL
            AND result_ref IS NULL
            AND completed_fencing_token IS NULL)
        OR
        (scoring_state = 'leased'
            AND attempt_count > 0
            AND next_attempt_at_unix_ms IS NULL
            AND result_ref IS NULL
            AND completed_fencing_token IS NULL)
        OR
        (scoring_state = 'retry_scheduled'
            AND attempt_count > 0
            AND attempt_count < max_attempts
            AND next_attempt_at_unix_ms IS NOT NULL
            AND last_failure_code IS NOT NULL
            AND result_ref IS NULL
            AND completed_fencing_token IS NULL)
        OR
        (scoring_state = 'completed'
            AND attempt_count > 0
            AND next_attempt_at_unix_ms IS NULL
            AND result_ref IS NOT NULL
            AND completed_fencing_token = attempt_count)
        OR
        (scoring_state = 'quarantined'
            AND attempt_count > 0
            AND next_attempt_at_unix_ms IS NULL
            AND last_failure_code IS NOT NULL
            AND result_ref IS NULL
            AND completed_fencing_token IS NULL)
        OR
        (scoring_state = 'cancelled'
            AND next_attempt_at_unix_ms IS NULL
            AND result_ref IS NULL
            AND completed_fencing_token IS NULL)
    )
)
$create_scoring_job_state$;
        relation_ref := to_regclass('scoring_job_state');
    END IF;

    IF relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state migration did not create its owned table';
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
        'scoring_job_ref:text:not_null',
        'scoring_request_ref:text:not_null',
        'scoring_state:text:not_null',
        'attempt_count:integer:not_null',
        'max_attempts:integer:not_null',
        'next_attempt_at_unix_ms:bigint:nullable',
        'last_failure_code:text:nullable',
        'active_worker_ref:text:nullable',
        'active_lease_ref:text:nullable',
        'active_fencing_token:bigint:nullable',
        'active_lease_expires_at_unix_ms:bigint:nullable',
        'result_ref:text:nullable',
        'completed_fencing_token:bigint:nullable',
        'created_at:timestamp with time zone:not_null',
        'updated_at:timestamp with time zone:not_null'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state column contract does not match migration 0002';
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

    IF actual_defaults IS DISTINCT FROM ARRAY[
        'attempt_count:0',
        'created_at:clock_timestamp()',
        'updated_at:clock_timestamp()'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state default contract does not match migration 0002';
    END IF;

    SELECT ARRAY(
        SELECT constraint_record.conname::TEXT
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraint_names;

    IF actual_constraint_names IS DISTINCT FROM ARRAY[
        'scoring_active_lease_shape_check',
        'scoring_attempt_budget_check',
        'scoring_attempt_count_nonnegative_check',
        'scoring_attempt_count_not_null',
        'scoring_completed_fence_positive_check',
        'scoring_created_at_not_null',
        'scoring_failure_code_format_check',
        'scoring_fencing_attempt_match_check',
        'scoring_fencing_token_positive_check',
        'scoring_job_ref_format_check',
        'scoring_job_ref_not_null',
        'scoring_job_state_pkey',
        'scoring_lease_expiry_positive_check',
        'scoring_lease_ref_format_check',
        'scoring_max_attempts_not_null',
        'scoring_max_attempts_positive_check',
        'scoring_next_attempt_positive_check',
        'scoring_request_ref_format_check',
        'scoring_request_ref_not_null',
        'scoring_result_ref_format_check',
        'scoring_state_not_null',
        'scoring_state_shape_check',
        'scoring_state_value_check',
        'scoring_updated_at_not_null',
        'scoring_worker_ref_format_check'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state constraint contract does not match migration 0002';
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

    actual_constraint_manifest := array_to_string(actual_constraints, E'\n');

    IF NOT created_table THEN
        stored_constraint_manifest := obj_description(relation_ref, 'pg_class');
        IF stored_constraint_manifest IS NULL
            OR left(stored_constraint_manifest, char_length(constraint_manifest_prefix))
                IS DISTINCT FROM constraint_manifest_prefix
            OR substring(
                stored_constraint_manifest
                FROM char_length(constraint_manifest_prefix) + 1
            ) IS DISTINCT FROM actual_constraint_manifest
        THEN
            RAISE EXCEPTION USING
                ERRCODE = '55000',
                MESSAGE = 'scoring_job_state constraint contract does not match migration 0002';
        END IF;

        SELECT
            COUNT(*) = 6
            AND bool_and(
                position('scoring_job_reference_is_valid' IN pg_get_constraintdef(oid)) > 0
            )
        INTO current_reference_contract
        FROM pg_constraint
        WHERE conrelid = relation_ref
          AND conname = ANY (ARRAY[
              'scoring_job_ref_format_check',
              'scoring_request_ref_format_check',
              'scoring_failure_code_format_check',
              'scoring_worker_ref_format_check',
              'scoring_lease_ref_format_check',
              'scoring_result_ref_format_check'
          ]);

        SELECT
            COUNT(*) = 6
            AND bool_and(
                position('btrim(' IN pg_get_constraintdef(oid)) > 0
                AND position('[[:digit:]]' IN pg_get_constraintdef(oid)) > 0
            )
        INTO legacy_reference_contract
        FROM pg_constraint
        WHERE conrelid = relation_ref
          AND conname = ANY (ARRAY[
              'scoring_job_ref_format_check',
              'scoring_request_ref_format_check',
              'scoring_failure_code_format_check',
              'scoring_worker_ref_format_check',
              'scoring_lease_ref_format_check',
              'scoring_result_ref_format_check'
          ]);

        IF NOT current_reference_contract AND NOT legacy_reference_contract THEN
            RAISE EXCEPTION USING
                ERRCODE = '55000',
                MESSAGE = 'scoring_job_state reference contract does not match migration 0002';
        END IF;
    END IF;

    -- Recreate all predicate-dependent checks even for the current contract. Replacing an
    -- immutable SQL helper does not by itself rescan already-validated rows, while DROP/ADD makes
    -- migration replay fail closed if a prior weakened helper admitted an invalid durable identity.
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_result_ref_format_check;
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_lease_ref_format_check;
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_worker_ref_format_check;
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_failure_code_format_check;
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_request_ref_format_check;
    ALTER TABLE scoring_job_state DROP CONSTRAINT scoring_job_ref_format_check;

    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_job_ref_format_check CHECK (
        scoring_job_reference_is_valid(scoring_job_ref)
    );
    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_request_ref_format_check CHECK (
        scoring_job_reference_is_valid(scoring_request_ref)
    );
    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_failure_code_format_check CHECK (
        last_failure_code IS NULL OR scoring_job_reference_is_valid(last_failure_code)
    );
    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_worker_ref_format_check CHECK (
        active_worker_ref IS NULL OR scoring_job_reference_is_valid(active_worker_ref)
    );
    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_lease_ref_format_check CHECK (
        active_lease_ref IS NULL OR scoring_job_reference_is_valid(active_lease_ref)
    );
    ALTER TABLE scoring_job_state ADD CONSTRAINT scoring_result_ref_format_check CHECK (
        result_ref IS NULL OR scoring_job_reference_is_valid(result_ref)
    );

    -- Recompute the manifest after an accepted legacy upgrade or current-contract revalidation and
    -- persist exactly what PostgreSQL is enforcing now.
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
    actual_constraint_manifest := array_to_string(actual_constraints, E'\n');

    BEGIN
        INSERT INTO scoring_job_state (
            scoring_job_ref,
            scoring_request_ref,
            scoring_state,
            attempt_count,
            max_attempts
        ) VALUES (
            probe_job_ref,
            probe_request_ref,
            'queued',
            1,
            3
        );
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state accepted an impossible queued state';
    EXCEPTION
        WHEN check_violation THEN NULL;
    END;

    EXECUTE format(
        'COMMENT ON TABLE %s IS %L',
        relation_ref,
        constraint_manifest_prefix || actual_constraint_manifest
    );
END
$scoring_job_schema$;
