CREATE TABLE IF NOT EXISTS anonymous_credential_evidence (
    credential_ref TEXT CONSTRAINT anonymous_credential_evidence_credential_ref_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_credential_ref_format_check CHECK (
            credential_ref = btrim(credential_ref)
            AND credential_ref <> ''
            AND NOT (
                credential_ref ~ '[[:digit:]]'
                AND credential_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    tenant_ref TEXT CONSTRAINT anonymous_credential_evidence_tenant_ref_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_tenant_ref_format_check CHECK (
            tenant_ref = btrim(tenant_ref)
            AND tenant_ref <> ''
            AND NOT (
                tenant_ref ~ '[[:digit:]]'
                AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT anonymous_credential_evidence_participant_ref_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT anonymous_credential_evidence_session_ref_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    proof_digest TEXT CONSTRAINT anonymous_credential_evidence_proof_digest_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_proof_digest_format_check CHECK (
            proof_digest = btrim(proof_digest)
            AND proof_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    issued_at_unix_ms BIGINT CONSTRAINT anonymous_credential_evidence_issued_at_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_issued_at_positive_check CHECK (
            issued_at_unix_ms > 0
        ),
    expires_at_unix_ms BIGINT CONSTRAINT anonymous_credential_evidence_expires_at_not_null NOT NULL
        CONSTRAINT anonymous_credential_evidence_lifetime_order_check CHECK (
            expires_at_unix_ms > issued_at_unix_ms
        ),
    revoked_at_unix_ms BIGINT
        CONSTRAINT anonymous_credential_evidence_revocation_order_check CHECK (
            revoked_at_unix_ms IS NULL
            OR (
                revoked_at_unix_ms > 0
                AND revoked_at_unix_ms >= issued_at_unix_ms
            )
        ),
    recorded_at TIMESTAMPTZ CONSTRAINT anonymous_credential_evidence_recorded_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT anonymous_credential_evidence_pkey PRIMARY KEY (credential_ref),
    CONSTRAINT anonymous_credential_evidence_proof_digest_unique UNIQUE (proof_digest)
);

CREATE OR REPLACE FUNCTION reject_anonymous_credential_identity_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'anonymous credential evidence cannot be deleted'
            USING ERRCODE = '55000';
    END IF;
    IF NEW.credential_ref IS DISTINCT FROM OLD.credential_ref
        OR NEW.tenant_ref IS DISTINCT FROM OLD.tenant_ref
        OR NEW.participant_ref IS DISTINCT FROM OLD.participant_ref
        OR NEW.session_ref IS DISTINCT FROM OLD.session_ref
        OR NEW.proof_digest IS DISTINCT FROM OLD.proof_digest
        OR NEW.issued_at_unix_ms IS DISTINCT FROM OLD.issued_at_unix_ms
        OR NEW.expires_at_unix_ms IS DISTINCT FROM OLD.expires_at_unix_ms
        OR NEW.recorded_at IS DISTINCT FROM OLD.recorded_at
    THEN
        RAISE EXCEPTION 'anonymous credential identity evidence is immutable'
            USING ERRCODE = '55000';
    END IF;
    IF OLD.revoked_at_unix_ms IS NOT NULL
        AND NEW.revoked_at_unix_ms IS DISTINCT FROM OLD.revoked_at_unix_ms
    THEN
        RAISE EXCEPTION 'anonymous credential revocation evidence cannot be replaced'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS anonymous_credential_evidence_identity_guard
    ON anonymous_credential_evidence;
CREATE TRIGGER anonymous_credential_evidence_identity_guard
    BEFORE UPDATE OR DELETE ON anonymous_credential_evidence
    FOR EACH ROW
    EXECUTE FUNCTION reject_anonymous_credential_identity_mutation();

CREATE OR REPLACE FUNCTION reject_anonymous_credential_evidence_truncate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'anonymous credential evidence cannot be truncated'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS anonymous_credential_evidence_truncate_guard
    ON anonymous_credential_evidence;
CREATE TRIGGER anonymous_credential_evidence_truncate_guard
    BEFORE TRUNCATE ON anonymous_credential_evidence
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_anonymous_credential_evidence_truncate();
