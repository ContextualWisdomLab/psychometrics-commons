CREATE TABLE IF NOT EXISTS tenant_account (
    tenant_ref TEXT CONSTRAINT tenant_account_tenant_ref_not_null NOT NULL
        CONSTRAINT tenant_account_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at TIMESTAMPTZ CONSTRAINT tenant_account_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT tenant_account_pkey PRIMARY KEY (tenant_ref)
);

CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT CONSTRAINT assessment_participant_participant_ref_not_null NOT NULL
        CONSTRAINT assessment_participant_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT CONSTRAINT assessment_participant_tenant_ref_not_null NOT NULL,
    created_at_unix_ms BIGINT CONSTRAINT assessment_participant_created_at_unix_not_null NOT NULL
        CONSTRAINT assessment_participant_created_at_unix_positive_check CHECK (
            created_at_unix_ms > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT assessment_participant_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT assessment_participant_pkey PRIMARY KEY (participant_ref),
    CONSTRAINT assessment_participant_tenant_ref_fkey FOREIGN KEY (tenant_ref)
        REFERENCES tenant_account (tenant_ref)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    CONSTRAINT assessment_participant_tenant_participant_key
        UNIQUE (tenant_ref, participant_ref)
);
