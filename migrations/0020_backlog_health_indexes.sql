-- Keep readiness probes bounded as durable terminal history grows.
-- data_rights_propagation_state already has the state/time index created by 0003.
CREATE INDEX IF NOT EXISTS integration_outbox_pending_health_idx
    ON integration_outbox (latest_event_at_unix_ms)
    WHERE current_state = 'pending';

CREATE INDEX IF NOT EXISTS integration_outbox_quarantined_health_idx
    ON integration_outbox (latest_event_at_unix_ms)
    WHERE current_state = 'quarantined';

CREATE INDEX IF NOT EXISTS integration_consumption_active_health_idx
    ON integration_consumption (latest_event_at_unix_ms)
    WHERE consumption_state IN ('pending', 'processing');

CREATE INDEX IF NOT EXISTS integration_consumption_quarantined_health_idx
    ON integration_consumption (latest_event_at_unix_ms)
    WHERE consumption_state = 'quarantined';

CREATE INDEX IF NOT EXISTS data_rights_request_active_health_idx
    ON data_rights_request_state (requested_at_unix_ms)
    WHERE current_state IN ('requested', 'identity_verified', 'processing');
