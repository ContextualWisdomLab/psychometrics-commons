-- Durable product-owned anonymous-first participant identity.
--
-- This table stores only the stable Psychometrics Commons participant base record.
-- Optional Keyverse link history remains a separate append-only identity-link concern.

CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL,

    CONSTRAINT assessment_participant_ref_format_check CHECK (
        participant_ref = btrim(participant_ref)
        AND participant_ref <> ''
        AND NOT (
            participant_ref ~ '[[:digit:]]'
            AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
        tenant_ref = btrim(tenant_ref)
        AND tenant_ref <> ''
        AND NOT (
            tenant_ref ~ '[[:digit:]]'
            AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT assessment_participant_created_time_positive_check CHECK (
        created_at_unix_ms > 0
    )
);
