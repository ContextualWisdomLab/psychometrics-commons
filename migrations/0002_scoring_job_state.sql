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
    constraint_manifest_prefix CONSTANT TEXT :=
        'psychometrics-commons:migration-0002:constraint-manifest:';
    probe_job_ref TEXT := 'scoring_job_migration_probe_' || pg_backend_pid()::TEXT;
    probe_request_ref TEXT := 'scoring_request_migration_probe_' || pg_backend_pid()::TEXT;
BEGIN
    IF created_table THEN
        EXECUTE $create_scoring_job_state$
CREATE TABLE scoring_job_state (
    scoring_job_ref TEXT NOT NULL
        CONSTRAINT scoring_job_ref_format_check CHECK (
            scoring_job_ref = btrim(scoring_job_ref)
            AND scoring_job_ref <> ''
            AND NOT (
                scoring_job_ref ~ '[[:digit:]]'
                AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_request_ref TEXT NOT NULL
        CONSTRAINT scoring_request_ref_format_check CHECK (
            scoring_request_ref = btrim(scoring_request_ref)
            AND scoring_request_ref <> ''
            AND NOT (
                scoring_request_ref ~ '[[:digit:]]'
                AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_state TEXT NOT NULL
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
    attempt_count INTEGER NOT NULL DEFAULT 0
        CONSTRAINT scoring_attempt_count_nonnegative_check CHECK (attempt_count >= 0),
    max_attempts INTEGER NOT NULL
        CONSTRAINT scoring_max_attempts_positive_check CHECK (max_attempts > 0),
    next_attempt_at_unix_ms BIGINT
        CONSTRAINT scoring_next_attempt_positive_check CHECK (next_attempt_at_unix_ms > 0),
    last_failure_code TEXT
        CONSTRAINT scoring_failure_code_format_check CHECK (
            last_failure_code IS NULL OR (
                last_failure_code = btrim(last_failure_code)
                AND last_failure_code <> ''
                AND NOT (
                    last_failure_code ~ '[[:digit:]]'
                    AND last_failure_code ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    active_worker_ref TEXT
        CONSTRAINT scoring_worker_ref_format_check CHECK (active_worker_ref IS NULL OR (
            active_worker_ref = btrim(active_worker_ref)
            AND active_worker_ref <> ''
            AND NOT (
                active_worker_ref ~ '[[:digit:]]'
                AND active_worker_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    active_lease_ref TEXT
        CONSTRAINT scoring_lease_ref_format_check CHECK (active_lease_ref IS NULL OR (
            active_lease_ref = btrim(active_lease_ref)
            AND active_lease_ref <> ''
            AND NOT (
                active_lease_ref ~ '[[:digit:]]'
                AND active_lease_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    active_fencing_token BIGINT,
    active_lease_expires_at_unix_ms BIGINT,
    result_ref TEXT,
    completed_fencing_token BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
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
    CONSTRAINT scoring_result_ref_format_check CHECK (result_ref IS NULL OR (
        result_ref = btrim(result_ref)
        AND result_ref <> ''
        AND NOT (result_ref ~ '[[:digit:]]' AND result_ref ~ '^[[:digit:]+,.eE-]+$')
    )),
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
          AND constraint_record.contype IN ('c', 'p')
          AND constraint_record.convalidated
          AND constraint_record.conenforced
        ORDER BY constraint_record.conname
    ) INTO actual_constraint_names;

    IF actual_constraint_names IS DISTINCT FROM ARRAY[
        'scoring_active_lease_shape_check',
        'scoring_attempt_budget_check',
        'scoring_attempt_count_nonnegative_check',
        'scoring_completed_fence_positive_check',
        'scoring_failure_code_format_check',
        'scoring_fencing_attempt_match_check',
        'scoring_fencing_token_positive_check',
        'scoring_job_ref_format_check',
        'scoring_job_state_pkey',
        'scoring_lease_expiry_positive_check',
        'scoring_lease_ref_format_check',
        'scoring_max_attempts_positive_check',
        'scoring_next_attempt_positive_check',
        'scoring_request_ref_format_check',
        'scoring_result_ref_format_check',
        'scoring_state_shape_check',
        'scoring_state_value_check',
        'scoring_worker_ref_format_check'
    ]::TEXT[] THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'scoring_job_state constraint contract does not match migration 0002';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            constraint_record.conname,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'p')
          AND constraint_record.convalidated
          AND constraint_record.conenforced
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
    END IF;

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

    IF created_table THEN
        EXECUTE format(
            'COMMENT ON TABLE %s IS %L',
            relation_ref,
            constraint_manifest_prefix || actual_constraint_manifest
        );
    END IF;
END
$scoring_job_schema$;
