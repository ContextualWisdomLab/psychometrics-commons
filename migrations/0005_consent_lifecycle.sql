CREATE TABLE IF NOT EXISTS consent_ledger (
    participant_ref TEXT CONSTRAINT consent_ledger_participant_ref_not_null NOT NULL
        CONSTRAINT consent_ledger_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at TIMESTAMPTZ CONSTRAINT consent_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT consent_ledger_pkey PRIMARY KEY (participant_ref)
);

CREATE TABLE IF NOT EXISTS consent_event (
    participant_ref TEXT CONSTRAINT consent_event_participant_ref_not_null NOT NULL,
    event_ref TEXT CONSTRAINT consent_event_event_ref_not_null NOT NULL
        CONSTRAINT consent_event_event_ref_format_check CHECK (
            event_ref = btrim(event_ref)
            AND event_ref <> ''
            AND NOT (
                event_ref ~ '[[:digit:]]'
                AND event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consent_purpose TEXT CONSTRAINT consent_event_purpose_not_null NOT NULL
        CONSTRAINT consent_event_purpose_value_check CHECK (
            consent_purpose IN (
                'service_operation',
                'account_persistence',
                'longitudinal_observation',
                'research_contribution',
                'communications'
            )
        ),
    consent_decision TEXT CONSTRAINT consent_event_decision_not_null NOT NULL
        CONSTRAINT consent_event_decision_value_check CHECK (
            consent_decision IN ('granted', 'revoked')
        ),
    consent_form_version_ref TEXT CONSTRAINT consent_event_form_ref_not_null NOT NULL
        CONSTRAINT consent_event_form_ref_format_check CHECK (
            consent_form_version_ref = btrim(consent_form_version_ref)
            AND consent_form_version_ref <> ''
            AND NOT (
                consent_form_version_ref ~ '[[:digit:]]'
                AND consent_form_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_scope_ref TEXT
        CONSTRAINT consent_event_research_scope_format_check CHECK (
            research_scope_ref IS NULL OR (
                research_scope_ref = btrim(research_scope_ref)
                AND research_scope_ref <> ''
                AND NOT (
                    research_scope_ref ~ '[[:digit:]]'
                    AND research_scope_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    occurred_at_unix_ms BIGINT CONSTRAINT consent_event_occurred_at_not_null NOT NULL
        CONSTRAINT consent_event_occurred_at_positive_check CHECK (occurred_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT consent_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT consent_event_pkey PRIMARY KEY (participant_ref, event_ref),
    CONSTRAINT consent_event_ledger_fk FOREIGN KEY (participant_ref)
        REFERENCES consent_ledger (participant_ref),
    CONSTRAINT consent_event_research_scope_shape_check CHECK (
        (consent_purpose = 'research_contribution' AND research_scope_ref IS NOT NULL)
        OR
        (consent_purpose <> 'research_contribution' AND research_scope_ref IS NULL)
    )
);
