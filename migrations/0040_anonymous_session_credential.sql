-- Durable short-lived anonymous assessment credential evidence.
-- Raw bearer proofs never enter this table; only canonical SHA-256 digests are persisted.
CREATE TABLE IF NOT EXISTS anonymous_session_credential (
    credential_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    participant_ref TEXT NOT NULL,
    session_ref TEXT NOT NULL,
    proof_digest TEXT NOT NULL,
    issued_at_unix_ms BIGINT NOT NULL,
    expires_at_unix_ms BIGINT NOT NULL,
    revoked_at_unix_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT anonymous_credential_ref_format_check CHECK (
        credential_ref = btrim(credential_ref)
        AND credential_ref <> ''
        AND NOT (
            credential_ref ~ '[[:digit:]]'
            AND credential_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT anonymous_credential_tenant_ref_format_check CHECK (
        tenant_ref = btrim(tenant_ref)
        AND tenant_ref <> ''
        AND NOT (
            tenant_ref ~ '[[:digit:]]'
            AND tenant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT anonymous_credential_participant_ref_format_check CHECK (
        participant_ref = btrim(participant_ref)
        AND participant_ref <> ''
        AND NOT (
            participant_ref ~ '[[:digit:]]'
            AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT anonymous_credential_session_ref_format_check CHECK (
        session_ref = btrim(session_ref)
        AND session_ref <> ''
        AND NOT (
            session_ref ~ '[[:digit:]]'
            AND session_ref ~ '^[[:digit:]+,.eE-]+$'
        )
    ),
    CONSTRAINT anonymous_credential_proof_digest_format_check CHECK (
        proof_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    CONSTRAINT anonymous_credential_proof_digest_unique UNIQUE (proof_digest),
    CONSTRAINT anonymous_credential_issue_time_check CHECK (issued_at_unix_ms > 0),
    CONSTRAINT anonymous_credential_expiry_time_check CHECK (expires_at_unix_ms > issued_at_unix_ms),
    CONSTRAINT anonymous_credential_revocation_time_check CHECK (
        revoked_at_unix_ms IS NULL OR revoked_at_unix_ms >= issued_at_unix_ms
    ),
    CONSTRAINT anonymous_credential_tenant_identity_unique UNIQUE (credential_ref, tenant_ref)
);

CREATE INDEX IF NOT EXISTS anonymous_credential_session_lookup_idx
    ON anonymous_session_credential (tenant_ref, participant_ref, session_ref);

CREATE OR REPLACE FUNCTION enforce_anonymous_credential_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.credential_ref IS DISTINCT FROM NEW.credential_ref
        OR OLD.tenant_ref IS DISTINCT FROM NEW.tenant_ref
        OR OLD.participant_ref IS DISTINCT FROM NEW.participant_ref
        OR OLD.session_ref IS DISTINCT FROM NEW.session_ref
        OR OLD.proof_digest IS DISTINCT FROM NEW.proof_digest
        OR OLD.issued_at_unix_ms IS DISTINCT FROM NEW.issued_at_unix_ms
        OR OLD.expires_at_unix_ms IS DISTINCT FROM NEW.expires_at_unix_ms
        OR OLD.created_at IS DISTINCT FROM NEW.created_at
    THEN
        RAISE EXCEPTION 'anonymous credential issue evidence is immutable';
    END IF;

    IF OLD.revoked_at_unix_ms IS NOT NULL
        OR NEW.revoked_at_unix_ms IS NULL
        OR NEW.revoked_at_unix_ms < OLD.issued_at_unix_ms
    THEN
        RAISE EXCEPTION 'anonymous credential revocation evidence is append-only';
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS anonymous_credential_update_guard ON anonymous_session_credential;
CREATE TRIGGER anonymous_credential_update_guard
BEFORE UPDATE ON anonymous_session_credential
FOR EACH ROW
EXECUTE FUNCTION enforce_anonymous_credential_update();

CREATE OR REPLACE FUNCTION reject_anonymous_credential_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'anonymous credential evidence cannot be deleted in place';
END;
$$;

DROP TRIGGER IF EXISTS anonymous_credential_delete_guard ON anonymous_session_credential;
CREATE TRIGGER anonymous_credential_delete_guard
BEFORE DELETE ON anonymous_session_credential
FOR EACH ROW
EXECUTE FUNCTION reject_anonymous_credential_delete();

DROP TRIGGER IF EXISTS anonymous_credential_truncate_guard ON anonymous_session_credential;
CREATE TRIGGER anonymous_credential_truncate_guard
BEFORE TRUNCATE ON anonymous_session_credential
FOR EACH STATEMENT
EXECUTE FUNCTION reject_anonymous_credential_delete();
