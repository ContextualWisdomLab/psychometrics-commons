-- Keep expired-lease readiness probes bounded as leased history is recovered.
CREATE INDEX IF NOT EXISTS scoring_job_state_leased_expiry_health_idx
    ON scoring_job_state (active_lease_expires_at_unix_ms)
    WHERE scoring_state = 'leased';
