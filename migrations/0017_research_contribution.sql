CREATE TABLE IF NOT EXISTS research_consent_snapshot (
    consent_snapshot_ref TEXT CONSTRAINT research_consent_snapshot_ref_not_null NOT NULL
        CONSTRAINT research_consent_snapshot_ref_format_check CHECK (
            consent_snapshot_ref = btrim(consent_snapshot_ref)
            AND consent_snapshot_ref <> ''
            AND NOT (
                consent_snapshot_ref ~ '[[:digit:]]'
                AND consent_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT research_consent_snapshot_participant_ref_not_null NOT NULL
        CONSTRAINT research_consent_snapshot_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_scope_ref TEXT CONSTRAINT research_consent_snapshot_scope_ref_not_null NOT NULL
        CONSTRAINT research_consent_snapshot_scope_ref_format_check CHECK (
            research_scope_ref = btrim(research_scope_ref)
            AND research_scope_ref <> ''
            AND NOT (
                research_scope_ref ~ '[[:digit:]]'
                AND research_scope_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consent_form_version_ref TEXT
        CONSTRAINT research_consent_snapshot_form_version_ref_not_null NOT NULL
        CONSTRAINT research_consent_snapshot_form_version_ref_format_check CHECK (
            consent_form_version_ref = btrim(consent_form_version_ref)
            AND consent_form_version_ref <> ''
            AND NOT (
                consent_form_version_ref ~ '[[:digit:]]'
                AND consent_form_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at TIMESTAMPTZ CONSTRAINT research_consent_snapshot_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_consent_snapshot_pkey PRIMARY KEY (consent_snapshot_ref),
    CONSTRAINT research_consent_snapshot_binding_unique UNIQUE (
        consent_snapshot_ref,
        participant_ref,
        research_scope_ref
    )
);

CREATE TABLE IF NOT EXISTS research_contribution (
    contribution_ref TEXT CONSTRAINT research_contribution_ref_not_null NOT NULL
        CONSTRAINT research_contribution_ref_format_check CHECK (
            contribution_ref = btrim(contribution_ref)
            AND contribution_ref <> ''
            AND NOT (
                contribution_ref ~ '[[:digit:]]'
                AND contribution_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT research_contribution_participant_ref_not_null NOT NULL
        CONSTRAINT research_contribution_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_participant_ref TEXT
        CONSTRAINT research_contribution_research_participant_ref_not_null NOT NULL
        CONSTRAINT research_contribution_research_participant_ref_format_check CHECK (
            research_participant_ref = btrim(research_participant_ref)
            AND research_participant_ref <> ''
            AND NOT (
                research_participant_ref ~ '[[:digit:]]'
                AND research_participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        )
        CONSTRAINT research_contribution_identity_separation_check CHECK (
            research_participant_ref <> participant_ref
        )
        CONSTRAINT research_contribution_research_participant_ref_unique UNIQUE,
    consent_snapshot_ref TEXT
        CONSTRAINT research_contribution_consent_snapshot_ref_not_null NOT NULL
        CONSTRAINT research_contribution_consent_snapshot_ref_format_check CHECK (
            consent_snapshot_ref = btrim(consent_snapshot_ref)
            AND consent_snapshot_ref <> ''
            AND NOT (
                consent_snapshot_ref ~ '[[:digit:]]'
                AND consent_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_scope_ref TEXT CONSTRAINT research_contribution_scope_ref_not_null NOT NULL
        CONSTRAINT research_contribution_scope_ref_format_check CHECK (
            research_scope_ref = btrim(research_scope_ref)
            AND research_scope_ref <> ''
            AND NOT (
                research_scope_ref ~ '[[:digit:]]'
                AND research_scope_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    started_at_unix_ms BIGINT CONSTRAINT research_contribution_started_at_not_null NOT NULL
        CONSTRAINT research_contribution_started_at_positive_check CHECK (started_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT research_contribution_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_contribution_pkey PRIMARY KEY (contribution_ref),
    CONSTRAINT research_contribution_consent_binding_fk FOREIGN KEY (
        consent_snapshot_ref,
        participant_ref,
        research_scope_ref
    ) REFERENCES research_consent_snapshot (
        consent_snapshot_ref,
        participant_ref,
        research_scope_ref
    )
);

CREATE TABLE IF NOT EXISTS research_withdrawal_event (
    contribution_ref TEXT CONSTRAINT research_withdrawal_contribution_ref_not_null NOT NULL,
    withdrawal_event_ref TEXT CONSTRAINT research_withdrawal_event_ref_not_null NOT NULL
        CONSTRAINT research_withdrawal_event_ref_format_check CHECK (
            withdrawal_event_ref = btrim(withdrawal_event_ref)
            AND withdrawal_event_ref <> ''
            AND NOT (
                withdrawal_event_ref ~ '[[:digit:]]'
                AND withdrawal_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    withdrawn_at_unix_ms BIGINT CONSTRAINT research_withdrawal_time_not_null NOT NULL
        CONSTRAINT research_withdrawal_time_positive_check CHECK (withdrawn_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT research_withdrawal_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_withdrawal_event_pkey PRIMARY KEY (contribution_ref),
    CONSTRAINT research_withdrawal_contribution_fk FOREIGN KEY (contribution_ref)
        REFERENCES research_contribution (contribution_ref),
    CONSTRAINT research_withdrawal_event_ref_unique UNIQUE (withdrawal_event_ref)
);

CREATE OR REPLACE FUNCTION reject_research_contribution_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'research contribution evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS research_consent_snapshot_immutable_guard
    ON research_consent_snapshot;
CREATE TRIGGER research_consent_snapshot_immutable_guard
    BEFORE UPDATE OR DELETE ON research_consent_snapshot
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();

DROP TRIGGER IF EXISTS research_consent_snapshot_truncate_guard
    ON research_consent_snapshot;
CREATE TRIGGER research_consent_snapshot_truncate_guard
    BEFORE TRUNCATE ON research_consent_snapshot
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();

DROP TRIGGER IF EXISTS research_contribution_immutable_guard
    ON research_contribution;
CREATE TRIGGER research_contribution_immutable_guard
    BEFORE UPDATE OR DELETE ON research_contribution
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();

DROP TRIGGER IF EXISTS research_contribution_truncate_guard
    ON research_contribution;
CREATE TRIGGER research_contribution_truncate_guard
    BEFORE TRUNCATE ON research_contribution
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();

DROP TRIGGER IF EXISTS research_withdrawal_event_immutable_guard
    ON research_withdrawal_event;
CREATE TRIGGER research_withdrawal_event_immutable_guard
    BEFORE UPDATE OR DELETE ON research_withdrawal_event
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();

DROP TRIGGER IF EXISTS research_withdrawal_event_truncate_guard
    ON research_withdrawal_event;
CREATE TRIGGER research_withdrawal_event_truncate_guard
    BEFORE TRUNCATE ON research_withdrawal_event
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_contribution_evidence_mutation();
