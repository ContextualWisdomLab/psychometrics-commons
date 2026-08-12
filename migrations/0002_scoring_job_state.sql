CREATE TABLE IF NOT EXISTS scoring_job_state (
    scoring_job_ref TEXT PRIMARY KEY
        CHECK (
            scoring_job_ref = btrim(scoring_job_ref)
            AND scoring_job_ref <> ''
            AND NOT (
                scoring_job_ref ~ '[[:digit:]]'
                AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_request_ref TEXT NOT NULL
        CHECK (
            scoring_request_ref = btrim(scoring_request_ref)
            AND scoring_request_ref <> ''
            AND NOT (
                scoring_request_ref ~ '[[:digit:]]'
                AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_state TEXT NOT NULL
        CHECK (
            scoring_state IN (
                'queued',
                'leased',
                'retry_scheduled',
                'completed',
                'quarantined',
                'cancelled'
            )
        ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    next_attempt_at_unix_ms BIGINT CHECK (next_attempt_at_unix_ms > 0),
    last_failure_code TEXT
        CHECK (
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
        CHECK (active_worker_ref IS NULL OR (
            active_worker_ref = btrim(active_worker_ref)
            AND active_worker_ref <> ''
            AND NOT (
                active_worker_ref ~ '[[:digit:]]'
                AND active_worker_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    active_lease_ref TEXT
        CHECK (active_lease_ref IS NULL OR (
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
    CHECK (attempt_count <= max_attempts),
    CHECK (
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
    CHECK (active_fencing_token IS NULL OR active_fencing_token > 0),
    CHECK (active_fencing_token IS NULL OR active_fencing_token = attempt_count),
    CHECK (active_lease_expires_at_unix_ms IS NULL OR active_lease_expires_at_unix_ms > 0),
    CHECK (completed_fencing_token IS NULL OR completed_fencing_token > 0),
    CHECK (result_ref IS NULL OR (
        result_ref = btrim(result_ref)
        AND result_ref <> ''
        AND NOT (result_ref ~ '[[:digit:]]' AND result_ref ~ '^[[:digit:]+,.eE-]+$')
    ))
);
