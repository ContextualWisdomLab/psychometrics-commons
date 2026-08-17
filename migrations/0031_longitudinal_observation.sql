-- Durable, immutable normalized longitudinal observation evidence.

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
        observation_record_ref = btrim(observation_record_ref) AND observation_record_ref <> '' AND observation_record_ref !~ '^[0-9]+$'
        AND tenant_ref = btrim(tenant_ref) AND tenant_ref <> '' AND tenant_ref !~ '^[0-9]+$'
        AND enrollment_ref = btrim(enrollment_ref) AND enrollment_ref <> '' AND enrollment_ref !~ '^[0-9]+$'
        AND source_system_ref = btrim(source_system_ref) AND source_system_ref <> '' AND source_system_ref !~ '^[0-9]+$'
        AND source_observation_ref = btrim(source_observation_ref) AND source_observation_ref <> '' AND source_observation_ref !~ '^[0-9]+$'
        AND construct_ref = btrim(construct_ref) AND construct_ref <> '' AND construct_ref !~ '^[0-9]+$'
        AND measure_ref = btrim(measure_ref) AND measure_ref <> '' AND measure_ref !~ '^[0-9]+$'
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
        membership_context_ref = btrim(membership_context_ref)
        AND membership_context_ref <> ''
        AND membership_context_ref !~ '^[0-9]+$'
    ),
    CONSTRAINT longitudinal_membership_weight_check CHECK (
        weight_parts_per_10_000 BETWEEN 1 AND 10000
    )
);

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

DROP TRIGGER IF EXISTS longitudinal_membership_total_check ON longitudinal_membership_share;
CREATE CONSTRAINT TRIGGER longitudinal_membership_total_check
AFTER INSERT ON longitudinal_membership_share
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_longitudinal_membership_total();
