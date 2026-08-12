CREATE TABLE IF NOT EXISTS scoring_job (
    scoring_job_ref TEXT PRIMARY KEY
        CHECK (
            scoring_job_ref = btrim(scoring_job_ref)
            AND scoring_job_ref <> ''
            AND NOT (
                scoring_job_ref ~ '[[:digit:]]'
                AND scoring_job_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_request_ref TEXT NOT NULL UNIQUE
        CHECK (
            scoring_request_ref = btrim(scoring_request_ref)
            AND scoring_request_ref <> ''
            AND NOT (
                scoring_request_ref ~ '[[:digit:]]'
                AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    current_state TEXT NOT NULL DEFAULT 'queued'
        CHECK (current_state IN ('queued', 'leased', 'completed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    active_worker_ref TEXT
        CHECK (active_worker_ref IS NULL OR (
            active_worker_ref = btrim(active_worker_ref)
            AND active_worker_ref <> ''
            AND NOT (
                active_worker_ref ~ '[[:digit:]]'
                AND active_worker_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    active_lease_ref TEXT UNIQUE
        CHECK (active_lease_ref IS NULL OR (
            active_lease_ref = btrim(active_lease_ref)
            AND active_lease_ref <> ''
            AND NOT (
                active_lease_ref ~ '[[:digit:]]'
                AND active_lease_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    active_fencing_token BIGINT CHECK (active_fencing_token IS NULL OR active_fencing_token > 0),
    lease_expires_at_unix_ms BIGINT CHECK (
        lease_expires_at_unix_ms IS NULL OR lease_expires_at_unix_ms > 0
    ),
    completed_lease_ref TEXT
        CHECK (completed_lease_ref IS NULL OR (
            completed_lease_ref = btrim(completed_lease_ref)
            AND completed_lease_ref <> ''
            AND NOT (
                completed_lease_ref ~ '[[:digit:]]'
                AND completed_lease_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    completed_fencing_token BIGINT
        CHECK (completed_fencing_token IS NULL OR completed_fencing_token > 0),
    scoring_result_ref TEXT
        CHECK (scoring_result_ref IS NULL OR (
            scoring_result_ref = btrim(scoring_result_ref)
            AND scoring_result_ref <> ''
            AND NOT (
                scoring_result_ref ~ '[[:digit:]]'
                AND scoring_result_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )),
    completed_at_unix_ms BIGINT CHECK (completed_at_unix_ms IS NULL OR completed_at_unix_ms > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CHECK (attempt_count <= max_attempts),
    CHECK (
        (current_state = 'queued'
            AND active_worker_ref IS NULL
            AND active_lease_ref IS NULL
            AND active_fencing_token IS NULL
            AND lease_expires_at_unix_ms IS NULL
            AND completed_lease_ref IS NULL
            AND completed_fencing_token IS NULL
            AND scoring_result_ref IS NULL
            AND completed_at_unix_ms IS NULL)
        OR
        (current_state = 'leased'
            AND active_worker_ref IS NOT NULL
            AND active_lease_ref IS NOT NULL
            AND active_fencing_token IS NOT NULL
            AND lease_expires_at_unix_ms IS NOT NULL
            AND active_fencing_token = attempt_count
            AND completed_lease_ref IS NULL
            AND completed_fencing_token IS NULL
            AND scoring_result_ref IS NULL
            AND completed_at_unix_ms IS NULL)
        OR
        (current_state = 'completed'
            AND active_worker_ref IS NULL
            AND active_lease_ref IS NULL
            AND active_fencing_token IS NULL
            AND lease_expires_at_unix_ms IS NULL
            AND completed_lease_ref IS NOT NULL
            AND completed_fencing_token IS NOT NULL
            AND completed_fencing_token = attempt_count
            AND scoring_result_ref IS NOT NULL
            AND completed_at_unix_ms IS NOT NULL)
    )
);
