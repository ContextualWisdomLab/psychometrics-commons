-- Durable, immutable normalized longitudinal observation evidence.

-- Keep the database boundary aligned with the shared opaque-reference contract: references must
-- be nonblank, have no outer whitespace, and must not be numeric-like (including signs,
-- decimal/group separators, or exponent notation). The fixed pg_catalog search path prevents
-- caller-controlled schemas from changing function resolution inside this CHECK helper.
CREATE OR REPLACE FUNCTION longitudinal_reference_is_valid(reference_value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $longitudinal_reference$
    SELECT
        reference_value IS NOT NULL
        AND reference_value <> ''
        AND left(reference_value, 1) !~ '[[:space:]]'
        AND right(reference_value, 1) !~ '[[:space:]]'
        AND NOT (
            reference_value ~ '[[:digit:]]'
            AND reference_value ~ '^[[:digit:]+,.eE-٫٬．，]+$'
        );
$longitudinal_reference$;

CREATE TABLE IF NOT EXISTS longitudinal_observation (
    observation_record_ref text PRIMARY KEY,
    tenant_ref text NOT NULL,
    enrollment_ref text NOT NULL,
    source_system_ref text NOT NULL,
    source_observation_ref text NOT NULL,
    construct_ref text NOT NULL,
    measure_ref text NOT NULL,
    validity_start_at_unix_ms bigint NOT NULL,
    validity_end_at_unix_ms bigint NOT NULL,
    recorded_at_unix_ms bigint NOT NULL,
    received_at_unix_ms bigint NOT NULL,
    ingested_at_unix_ms bigint NOT NULL,
    timezone_name text NOT NULL,
    utc_offset_minutes smallint NOT NULL,
    clock_anomaly_code text,
    CONSTRAINT longitudinal_observation_source_identity_unique
        UNIQUE (tenant_ref, enrollment_ref, source_system_ref, source_observation_ref),
    CONSTRAINT longitudinal_observation_reference_check CHECK (
        longitudinal_reference_is_valid(observation_record_ref)
        AND longitudinal_reference_is_valid(tenant_ref)
        AND longitudinal_reference_is_valid(enrollment_ref)
        AND longitudinal_reference_is_valid(source_system_ref)
        AND longitudinal_reference_is_valid(source_observation_ref)
        AND longitudinal_reference_is_valid(construct_ref)
        AND longitudinal_reference_is_valid(measure_ref)
    ),
    CONSTRAINT longitudinal_observation_time_check CHECK (
        validity_start_at_unix_ms > 0 AND validity_end_at_unix_ms >= validity_start_at_unix_ms
        AND recorded_at_unix_ms > 0 AND received_at_unix_ms > 0 AND ingested_at_unix_ms >= received_at_unix_ms
    ),
    CONSTRAINT longitudinal_observation_timezone_check CHECK (
        timezone_name = btrim(timezone_name) AND timezone_name <> '' AND timezone_name !~ '^[0-9]+$'
        AND utc_offset_minutes BETWEEN -720 AND 840
    ),
    CONSTRAINT longitudinal_observation_anomaly_check CHECK (
        clock_anomaly_code IS NULL OR clock_anomaly_code = 'recorded_after_received'
    )
);

CREATE TABLE IF NOT EXISTS longitudinal_membership_share (
    observation_record_ref text NOT NULL
        REFERENCES longitudinal_observation(observation_record_ref) ON DELETE RESTRICT,
    membership_sequence bigint NOT NULL,
    membership_context_ref text NOT NULL,
    weight_parts_per_10_000 integer NOT NULL,
    PRIMARY KEY (observation_record_ref, membership_sequence),
    CONSTRAINT longitudinal_membership_context_unique
        UNIQUE (observation_record_ref, membership_context_ref),
    CONSTRAINT longitudinal_membership_sequence_check CHECK (membership_sequence > 0),
    CONSTRAINT longitudinal_membership_reference_check CHECK (
        longitudinal_reference_is_valid(membership_context_ref)
    ),
    CONSTRAINT longitudinal_membership_weight_check CHECK (
        weight_parts_per_10_000 BETWEEN 1 AND 10000
    )
);

-- Reapply evolving CHECK definitions so an idempotent rerun upgrades a schema created by an
-- earlier iteration of this not-yet-shipped migration rather than trusting constraint names alone.
ALTER TABLE longitudinal_observation
    DROP CONSTRAINT IF EXISTS longitudinal_observation_reference_check;
ALTER TABLE longitudinal_observation
    ADD CONSTRAINT longitudinal_observation_reference_check CHECK (
        longitudinal_reference_is_valid(observation_record_ref)
        AND longitudinal_reference_is_valid(tenant_ref)
        AND longitudinal_reference_is_valid(enrollment_ref)
        AND longitudinal_reference_is_valid(source_system_ref)
        AND longitudinal_reference_is_valid(source_observation_ref)
        AND longitudinal_reference_is_valid(construct_ref)
        AND longitudinal_reference_is_valid(measure_ref)
    );

ALTER TABLE longitudinal_observation
    DROP CONSTRAINT IF EXISTS longitudinal_observation_anomaly_check;
ALTER TABLE longitudinal_observation
    ADD CONSTRAINT longitudinal_observation_anomaly_check CHECK (
        clock_anomaly_code IS NULL OR clock_anomaly_code = 'recorded_after_received'
    );

ALTER TABLE longitudinal_membership_share
    DROP CONSTRAINT IF EXISTS longitudinal_membership_reference_check;
ALTER TABLE longitudinal_membership_share
    ADD CONSTRAINT longitudinal_membership_reference_check CHECK (
        longitudinal_reference_is_valid(membership_context_ref)
    );

-- Existing rows from a partial pre-merge rollout must already satisfy the clock/code relation.
-- Fail the migration before installing the insert guard if they do not.
DO $longitudinal_anomaly_preflight$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM longitudinal_observation
        WHERE NOT (
            (clock_anomaly_code IS NULL AND recorded_at_unix_ms <= received_at_unix_ms)
            OR (
                clock_anomaly_code = 'recorded_after_received'
                AND recorded_at_unix_ms > received_at_unix_ms
            )
        )
    ) THEN
        RAISE EXCEPTION 'stored longitudinal observation clock anomaly evidence is inconsistent'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'longitudinal_observation_anomaly_check';
    END IF;
END;
$longitudinal_anomaly_preflight$;

-- Legitimate rows are append-only. Enforce the clock/code relation on INSERT; UPDATE is already
-- prohibited by the immutable-row trigger below. Keeping the consistency guard INSERT-only also
-- lets corruption-recovery tests deliberately disable immutability and prove loaders fail closed.
CREATE OR REPLACE FUNCTION validate_longitudinal_observation_anomaly_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF (
        (NEW.clock_anomaly_code IS NULL AND NEW.recorded_at_unix_ms <= NEW.received_at_unix_ms)
        OR (
            NEW.clock_anomaly_code = 'recorded_after_received'
            AND NEW.recorded_at_unix_ms > NEW.received_at_unix_ms
        )
    ) THEN
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'longitudinal observation clock anomaly code does not match clock order'
        USING ERRCODE = '23514',
              CONSTRAINT = 'longitudinal_observation_anomaly_check';
END;
$$;

DROP TRIGGER IF EXISTS longitudinal_observation_anomaly_insert ON longitudinal_observation;
CREATE TRIGGER longitudinal_observation_anomaly_insert
BEFORE INSERT ON longitudinal_observation
FOR EACH ROW EXECUTE FUNCTION validate_longitudinal_observation_anomaly_insert();

CREATE OR REPLACE FUNCTION reject_longitudinal_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'longitudinal observation evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS longitudinal_observation_immutable_update ON longitudinal_observation;
CREATE TRIGGER longitudinal_observation_immutable_update
BEFORE UPDATE OR DELETE ON longitudinal_observation
FOR EACH ROW EXECUTE FUNCTION reject_longitudinal_mutation();

DROP TRIGGER IF EXISTS longitudinal_observation_immutable_truncate ON longitudinal_observation;
CREATE TRIGGER longitudinal_observation_immutable_truncate
BEFORE TRUNCATE ON longitudinal_observation
FOR EACH STATEMENT EXECUTE FUNCTION reject_longitudinal_mutation();

DROP TRIGGER IF EXISTS longitudinal_membership_immutable_update ON longitudinal_membership_share;
CREATE TRIGGER longitudinal_membership_immutable_update
BEFORE UPDATE OR DELETE ON longitudinal_membership_share
FOR EACH ROW EXECUTE FUNCTION reject_longitudinal_mutation();

DROP TRIGGER IF EXISTS longitudinal_membership_immutable_truncate ON longitudinal_membership_share;
CREATE TRIGGER longitudinal_membership_immutable_truncate
BEFORE TRUNCATE ON longitudinal_membership_share
FOR EACH STATEMENT EXECUTE FUNCTION reject_longitudinal_mutation();

CREATE OR REPLACE FUNCTION enforce_longitudinal_membership_total()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    total_weight bigint;
BEGIN
    SELECT COALESCE(SUM(weight_parts_per_10_000), 0)
      INTO total_weight
      FROM longitudinal_membership_share
     WHERE observation_record_ref = NEW.observation_record_ref;
    IF total_weight <> 10000 THEN
        RAISE EXCEPTION 'longitudinal membership shares must sum to 10000'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

-- A membership-only trigger cannot observe an observation whose membership vector is empty,
-- because no child INSERT exists to fire it. Defer the same invariant from the parent INSERT so
-- the complete vector can be inserted in the transaction while a header-only commit still fails.
DROP TRIGGER IF EXISTS longitudinal_observation_membership_total_check ON longitudinal_observation;
CREATE CONSTRAINT TRIGGER longitudinal_observation_membership_total_check
AFTER INSERT ON longitudinal_observation
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_longitudinal_membership_total();

DROP TRIGGER IF EXISTS longitudinal_membership_total_check ON longitudinal_membership_share;
CREATE CONSTRAINT TRIGGER longitudinal_membership_total_check
AFTER INSERT ON longitudinal_membership_share
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_longitudinal_membership_total();
