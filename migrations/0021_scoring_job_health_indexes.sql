-- Keep scoring-job readiness probes bounded as terminal history grows.
CREATE INDEX IF NOT EXISTS scoring_job_state_active_health_idx
    ON scoring_job_state (created_at)
    WHERE scoring_state IN ('queued', 'leased', 'retry_scheduled');

CREATE INDEX IF NOT EXISTS scoring_job_state_quarantined_health_idx
    ON scoring_job_state (created_at)
    WHERE scoring_state = 'quarantined';
