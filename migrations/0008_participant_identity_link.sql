CREATE TABLE IF NOT EXISTS participant_identity_ledger (
    participant_ref TEXT CONSTRAINT participant_identity_ledger_participant_ref_not_null NOT NULL
        CONSTRAINT participant_identity_ledger_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT CONSTRAINT participant_identity_ledger_tenant_ref_not_null NOT NULL
        CONSTRAINT participant_identity_ledger_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    created_at_unix_ms BIGINT CONSTRAINT participant_identity_ledger_created_at_unix_not_null NOT NULL
        CONSTRAINT participant_identity_ledger_created_at_unix_positive_check CHECK (created_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT participant_identity_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT participant_identity_ledger_pkey PRIMARY KEY (participant_ref)
);

CREATE TABLE IF NOT EXISTS participant_identity_link_event (
    participant_ref TEXT CONSTRAINT participant_identity_link_event_participant_ref_not_null NOT NULL,
    link_event_ref TEXT CONSTRAINT participant_identity_link_event_link_event_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_link_event_ref_format_check CHECK (
            link_event_ref = btrim(link_event_ref)
            AND link_event_ref <> ''
            AND NOT (
                link_event_ref ~ '[[:digit:]]'
                AND link_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    issuer_ref TEXT CONSTRAINT participant_identity_link_event_issuer_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_issuer_ref_format_check CHECK (
            issuer_ref = btrim(issuer_ref)
            AND issuer_ref <> ''
            AND NOT (
                issuer_ref ~ '[[:digit:]]'
                AND issuer_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    subject_ref TEXT CONSTRAINT participant_identity_link_event_subject_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_subject_ref_format_check CHECK (
            subject_ref = btrim(subject_ref)
            AND subject_ref <> ''
            AND NOT (
                subject_ref ~ '[[:digit:]]'
                AND subject_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    anonymous_proof_ref TEXT CONSTRAINT participant_identity_link_event_anonymous_proof_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_anonymous_proof_format_check CHECK (
            anonymous_proof_ref = btrim(anonymous_proof_ref)
            AND anonymous_proof_ref <> ''
            AND NOT (
                anonymous_proof_ref ~ '[[:digit:]]'
                AND anonymous_proof_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    authenticated_proof_ref TEXT CONSTRAINT participant_identity_link_event_authenticated_proof_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_authenticated_proof_format_check CHECK (
            authenticated_proof_ref = btrim(authenticated_proof_ref)
            AND authenticated_proof_ref <> ''
            AND NOT (
                authenticated_proof_ref ~ '[[:digit:]]'
                AND authenticated_proof_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    linked_at_unix_ms BIGINT CONSTRAINT participant_identity_link_event_linked_at_unix_not_null NOT NULL
        CONSTRAINT participant_identity_link_event_linked_at_unix_positive_check CHECK (linked_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT participant_identity_link_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT participant_identity_link_event_pkey PRIMARY KEY (participant_ref, link_event_ref),
    CONSTRAINT participant_identity_link_event_ledger_fkey
        FOREIGN KEY (participant_ref) REFERENCES participant_identity_ledger (participant_ref)
);

CREATE TABLE IF NOT EXISTS participant_identity_link_end_event (
    participant_ref TEXT CONSTRAINT participant_identity_link_end_event_participant_ref_not_null NOT NULL,
    link_end_event_ref TEXT CONSTRAINT participant_identity_link_end_event_end_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_event_end_ref_format_check CHECK (
            link_end_event_ref = btrim(link_end_event_ref)
            AND link_end_event_ref <> ''
            AND NOT (
                link_end_event_ref ~ '[[:digit:]]'
                AND link_end_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    linked_event_ref TEXT CONSTRAINT participant_identity_link_end_event_linked_event_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_event_linked_event_format_check CHECK (
            linked_event_ref = btrim(linked_event_ref)
            AND linked_event_ref <> ''
            AND NOT (
                linked_event_ref ~ '[[:digit:]]'
                AND linked_event_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    evidence_ref TEXT CONSTRAINT participant_identity_link_end_event_evidence_ref_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_event_evidence_ref_format_check CHECK (
            evidence_ref = btrim(evidence_ref)
            AND evidence_ref <> ''
            AND NOT (
                evidence_ref ~ '[[:digit:]]'
                AND evidence_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    ended_at_unix_ms BIGINT CONSTRAINT participant_identity_link_end_event_ended_at_unix_not_null NOT NULL
        CONSTRAINT participant_identity_link_end_event_ended_at_unix_positive_check CHECK (ended_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT participant_identity_link_end_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT participant_identity_link_end_event_pkey PRIMARY KEY (participant_ref, link_end_event_ref),
    CONSTRAINT participant_identity_link_end_event_ledger_fkey
        FOREIGN KEY (participant_ref) REFERENCES participant_identity_ledger (participant_ref),
    CONSTRAINT participant_identity_link_end_event_link_fkey
        FOREIGN KEY (participant_ref, linked_event_ref)
        REFERENCES participant_identity_link_event (participant_ref, link_event_ref)
);
