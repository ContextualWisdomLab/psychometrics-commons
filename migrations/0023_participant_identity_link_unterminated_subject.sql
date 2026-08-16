-- Keep one unterminated issuer-scoped subject even when the derived
-- current_participant_identity_link projection is missing after restore
-- or operator repair. History remains append-only; this guard rejects a
-- second open link for the same tenant, issuer, and subject.

CREATE OR REPLACE FUNCTION reject_second_unterminated_identity_subject()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtext(
            NEW.tenant_ref
            || chr(31)
            || NEW.identity_issuer
        ),
        hashtext(NEW.identity_subject_ref)
    );
    IF EXISTS (
        SELECT 1
        FROM participant_identity_link existing_link
        WHERE existing_link.tenant_ref = NEW.tenant_ref
          AND existing_link.identity_issuer = NEW.identity_issuer
          AND existing_link.identity_subject_ref = NEW.identity_subject_ref
          AND existing_link.identity_link_ref <> NEW.identity_link_ref
          AND NOT EXISTS (
              SELECT 1
              FROM participant_identity_link_end existing_end
              WHERE existing_end.linked_event_ref = existing_link.identity_link_ref
          )
    ) THEN
        RAISE EXCEPTION
            'unterminated issuer-scoped subject already has a participant identity link'
            USING ERRCODE = '23505',
                  CONSTRAINT = 'participant_identity_link_unterminated_subject_unique';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS participant_identity_link_unterminated_subject_guard
    ON participant_identity_link;
CREATE TRIGGER participant_identity_link_unterminated_subject_guard
    BEFORE INSERT ON participant_identity_link
    FOR EACH ROW
    EXECUTE FUNCTION reject_second_unterminated_identity_subject();
