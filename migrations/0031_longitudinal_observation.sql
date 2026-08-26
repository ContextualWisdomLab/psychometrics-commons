-- Durable, immutable normalized longitudinal observation evidence.

-- Keep the database boundary aligned with Rust 1.97's Unicode 17 char::is_numeric contract.
-- PostgreSQL POSIX [[:digit:]] covers decimal digits but does not prove parity for Unicode letter
-- numbers and other numbers such as Roman numerals, superscripts, and vulgar fractions. The
-- generated int4multirange below is the exact Unicode 17 numeric code-point set used by rustc 1.97.
-- Mixed opaque identifiers remain valid because a value is numeric-like only when it contains at
-- least one numeric code point and every character is numeric or an allowed numeric spelling token.
-- The fixed pg_catalog search path prevents caller-controlled schemas from changing function
-- resolution inside this CHECK helper.
CREATE OR REPLACE FUNCTION longitudinal_reference_is_valid(reference_value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $longitudinal_reference$
    WITH reference_character AS (
        SELECT substr(reference_value, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_value)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            ascii(character_text) <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange
                AS is_numeric
        FROM reference_character
    )
    SELECT
        reference_value IS NOT NULL
        AND reference_value <> ''
        AND left(reference_value, 1) !~ '[[:space:]]'
        AND right(reference_value, 1) !~ '[[:space:]]'
        AND reference_value !~ '[[:cntrl:]]'
        AND NOT COALESCE(
            bool_or(is_numeric)
            AND bool_and(
                is_numeric
                OR character_text = ANY (
                    ARRAY[
                        '+',
                        '-',
                        '.',
                        ',',
                        'e',
                        'E',
                        U&'\066B',
                        U&'\066C',
                        U&'\FF0E',
                        U&'\FF0C'
                    ]
                )
            ),
            FALSE
        )
    FROM reference_classification;
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
        (clock_anomaly_code IS NULL AND recorded_at_unix_ms <= received_at_unix_ms)
        OR (
            clock_anomaly_code = 'recorded_after_received'
            AND recorded_at_unix_ms > received_at_unix_ms
        )
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
        (clock_anomaly_code IS NULL AND recorded_at_unix_ms <= received_at_unix_ms)
        OR (
            clock_anomaly_code = 'recorded_after_received'
            AND recorded_at_unix_ms > received_at_unix_ms
        )
    ) NOT VALID;

ALTER TABLE longitudinal_membership_share
    DROP CONSTRAINT IF EXISTS longitudinal_membership_reference_check;
ALTER TABLE longitudinal_membership_share
    ADD CONSTRAINT longitudinal_membership_reference_check CHECK (
        longitudinal_reference_is_valid(membership_context_ref)
    );

-- Existing rows from a partial pre-merge rollout must already satisfy the clock/code relation.
-- Fail the migration with an operator-readable error before validating the strengthened CHECK.
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

ALTER TABLE longitudinal_observation
    VALIDATE CONSTRAINT longitudinal_observation_anomaly_check;

-- Legitimate rows are append-only. Keep the INSERT guard as a named, early diagnostic in addition
-- to the CHECK constraint; the CHECK remains authoritative even if immutability is disabled for an
-- operator repair or integrity exercise.
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