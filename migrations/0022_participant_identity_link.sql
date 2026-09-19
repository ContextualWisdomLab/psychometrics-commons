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
    tenant_ref TEXT CONSTRAINT assessment_participant_tenant_ref_not_null NOT NULL
        CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at_unix_ms BIGINT CONSTRAINT assessment_participant_created_at_unix_ms_not_null NOT NULL
        CONSTRAINT assessment_participant_created_at_unix_ms_positive_check CHECK (
            created_at_unix_ms > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT assessment_participant_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT assessment_participant_pkey PRIMARY KEY (participant_ref)
);

CREATE TABLE IF NOT EXISTS participant_identity_link (
    identity_link_ref TEXT CONSTRAINT participant_identity_link_identity_link_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_identity_link_ref_format_check CHECK (
            identity_link_ref = btrim(identity_link_ref)
            AND identity_link_ref <> ''
            AND NOT (
                identity_link_ref ~ '[[:digit:]]'
                AND identity_link_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT participant_identity_link_participant_ref_not_null NOT NULL,
    tenant_ref TEXT CONSTRAINT participant_identity_link_tenant_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    identity_issuer TEXT CONSTRAINT participant_identity_link_identity_issuer_not_null NOT NULL
        CONSTRAINT participant_identity_link_identity_issuer_format_check CHECK (
            identity_issuer = btrim(identity_issuer)
            AND identity_issuer <> ''
            AND NOT (
                identity_issuer ~ '[[:digit:]]'
                AND identity_issuer ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    identity_subject_ref TEXT CONSTRAINT participant_identity_link_identity_subject_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_identity_subject_ref_format_check CHECK (
            identity_subject_ref = btrim(identity_subject_ref)
            AND identity_subject_ref <> ''
            AND NOT (
                identity_subject_ref ~ '[[:digit:]]'
                AND identity_subject_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    anonymous_proof_ref TEXT CONSTRAINT participant_identity_link_anonymous_proof_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_anonymous_proof_ref_format_check CHECK (
            anonymous_proof_ref = btrim(anonymous_proof_ref)
            AND anonymous_proof_ref <> ''
            AND NOT (
                anonymous_proof_ref ~ '[[:digit:]]'
                AND anonymous_proof_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    authenticated_proof_ref TEXT CONSTRAINT participant_identity_link_authenticated_proof_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_authenticated_proof_ref_format_check CHECK (
            authenticated_proof_ref = btrim(authenticated_proof_ref)
            AND authenticated_proof_ref <> ''
            AND NOT (
                authenticated_proof_ref ~ '[[:digit:]]'
                AND authenticated_proof_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    linked_at_unix_ms BIGINT CONSTRAINT participant_identity_link_linked_at_unix_ms_not_null NOT NULL
        CONSTRAINT participant_identity_link_linked_at_unix_ms_positive_check CHECK (
            linked_at_unix_ms > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT participant_identity_link_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT participant_identity_link_pkey PRIMARY KEY (identity_link_ref),
    CONSTRAINT participant_identity_link_participant_fk FOREIGN KEY (participant_ref)
        REFERENCES assessment_participant (participant_ref),
    CONSTRAINT participant_identity_link_participant_link_unique UNIQUE (
        participant_ref,
        identity_link_ref
    ),
    CONSTRAINT participant_identity_link_distinct_proofs_check CHECK (
        anonymous_proof_ref <> authenticated_proof_ref
    )
);

CREATE TABLE IF NOT EXISTS participant_identity_link_end (
    link_end_event_ref TEXT CONSTRAINT participant_identity_link_end_link_end_event_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_link_end_event_ref_format_check CHECK (
            link_end_event_ref = btrim(link_end_event_ref)
            AND link_end_event_ref <> ''
            AND NOT (
                link_end_event_ref ~ '[[:digit:]]'
                AND link_end_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT participant_identity_link_end_participant_ref_not_null NOT NULL,
    linked_event_ref TEXT CONSTRAINT participant_identity_link_end_linked_event_ref_not_null NOT NULL,
    evidence_ref TEXT CONSTRAINT participant_identity_link_end_evidence_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_evidence_ref_format_check CHECK (
            evidence_ref = btrim(evidence_ref)
            AND evidence_ref <> ''
            AND NOT (
                evidence_ref ~ '[[:digit:]]'
                AND evidence_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    ended_at_unix_ms BIGINT CONSTRAINT participant_identity_link_end_ended_at_unix_ms_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_ended_at_unix_ms_positive_check CHECK (
            ended_at_unix_ms > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT participant_identity_link_end_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT participant_identity_link_end_pkey PRIMARY KEY (link_end_event_ref),
    CONSTRAINT participant_identity_link_end_participant_fk FOREIGN KEY (participant_ref)
        REFERENCES assessment_participant (participant_ref),
    CONSTRAINT participant_identity_link_end_linked_event_fk FOREIGN KEY (linked_event_ref)
        REFERENCES participant_identity_link (identity_link_ref),
    CONSTRAINT participant_identity_link_end_linked_event_participant_fk
        FOREIGN KEY (participant_ref, linked_event_ref)
        REFERENCES participant_identity_link (participant_ref, identity_link_ref),
    CONSTRAINT participant_identity_link_end_linked_event_unique UNIQUE (linked_event_ref)
);

CREATE TABLE IF NOT EXISTS current_participant_identity_link (
    participant_ref TEXT CONSTRAINT current_participant_identity_link_participant_ref_not_null NOT NULL,
    identity_link_ref TEXT CONSTRAINT current_participant_identity_link_identity_link_ref_not_null NOT NULL,
    tenant_ref TEXT CONSTRAINT current_participant_identity_link_tenant_ref_not_null NOT NULL,
    identity_issuer TEXT CONSTRAINT current_participant_identity_link_identity_issuer_not_null NOT NULL,
    identity_subject_ref TEXT CONSTRAINT current_participant_identity_link_identity_subject_ref_not_null NOT NULL,
    CONSTRAINT current_participant_identity_link_pkey PRIMARY KEY (participant_ref),
    CONSTRAINT current_participant_identity_link_identity_fk FOREIGN KEY (identity_link_ref)
        REFERENCES participant_identity_link (identity_link_ref),
    CONSTRAINT current_participant_identity_link_identity_participant_fk
        FOREIGN KEY (participant_ref, identity_link_ref)
        REFERENCES participant_identity_link (participant_ref, identity_link_ref),
    CONSTRAINT current_participant_identity_link_participant_fk FOREIGN KEY (participant_ref)
        REFERENCES assessment_participant (participant_ref),
    CONSTRAINT current_participant_identity_link_subject_unique UNIQUE (
        tenant_ref,
        identity_issuer,
        identity_subject_ref
    )
);

CREATE INDEX IF NOT EXISTS participant_identity_link_current_subject_lookup
    ON participant_identity_link (tenant_ref, identity_issuer, identity_subject_ref);
