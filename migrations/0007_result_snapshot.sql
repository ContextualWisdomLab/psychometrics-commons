CREATE OR REPLACE FUNCTION result_snapshot_consent_refs_are_valid(reference_array TEXT[])
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $result_snapshot_consent_refs$
    SELECT
        NOT EXISTS (
            SELECT 1
            FROM unnest(reference_array) AS consent_reference(reference_text)
            WHERE reference_text IS NULL
                OR reference_text <> btrim(reference_text)
                OR reference_text = ''
                OR (
                    reference_text ~ '[[:digit:]]'
                    AND reference_text ~ '^[[:digit:]+,.eE-]+$'
                )
        )
        AND cardinality(reference_array) = (
            SELECT count(DISTINCT reference_text)
            FROM unnest(reference_array) AS consent_reference(reference_text)
        );
$result_snapshot_consent_refs$;

CREATE TABLE IF NOT EXISTS result_snapshot (
    result_snapshot_ref TEXT CONSTRAINT result_snapshot_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_ref_format_check CHECK (
            result_snapshot_ref = btrim(result_snapshot_ref)
            AND result_snapshot_ref <> ''
            AND NOT (
                result_snapshot_ref ~ '[[:digit:]]'
                AND result_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    participant_ref TEXT CONSTRAINT result_snapshot_participant_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_participant_ref_format_check CHECK (
            participant_ref = btrim(participant_ref)
            AND participant_ref <> ''
            AND NOT (
                participant_ref ~ '[[:digit:]]'
                AND participant_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_result_ref TEXT CONSTRAINT result_snapshot_scoring_result_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_scoring_result_ref_format_check CHECK (
            scoring_result_ref = btrim(scoring_result_ref)
            AND scoring_result_ref <> ''
            AND NOT (
                scoring_result_ref ~ '[[:digit:]]'
                AND scoring_result_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT result_snapshot_session_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    response_snapshot_ref TEXT CONSTRAINT result_snapshot_response_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_response_ref_format_check CHECK (
            response_snapshot_ref = btrim(response_snapshot_ref)
            AND response_snapshot_ref <> ''
            AND NOT (
                response_snapshot_ref ~ '[[:digit:]]'
                AND response_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    assessment_spec_ref TEXT CONSTRAINT result_snapshot_spec_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_spec_ref_format_check CHECK (
            assessment_spec_ref = btrim(assessment_spec_ref)
            AND assessment_spec_ref <> ''
            AND NOT (
                assessment_spec_ref ~ '[[:digit:]]'
                AND assessment_spec_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_version_ref TEXT CONSTRAINT result_snapshot_instrument_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_instrument_ref_format_check CHECK (
            instrument_version_ref = btrim(instrument_version_ref)
            AND instrument_version_ref <> ''
            AND NOT (
                instrument_version_ref ~ '[[:digit:]]'
                AND instrument_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_version_ref TEXT CONSTRAINT result_snapshot_scoring_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_scoring_ref_format_check CHECK (
            scoring_version_ref = btrim(scoring_version_ref)
            AND scoring_version_ref <> ''
            AND NOT (
                scoring_version_ref ~ '[[:digit:]]'
                AND scoring_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    calibration_reference TEXT CONSTRAINT result_snapshot_calibration_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_calibration_ref_format_check CHECK (
            calibration_reference = btrim(calibration_reference)
            AND calibration_reference <> ''
            AND NOT (
                calibration_reference ~ '[[:digit:]]'
                AND calibration_reference ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    norm_version_ref TEXT
        CONSTRAINT result_snapshot_norm_ref_format_check CHECK (
            norm_version_ref IS NULL OR (
                norm_version_ref = btrim(norm_version_ref)
                AND norm_version_ref <> ''
                AND NOT (
                    norm_version_ref ~ '[[:digit:]]'
                    AND norm_version_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    requested_output_schema_version INTEGER
        CONSTRAINT result_snapshot_schema_version_not_null NOT NULL
        CONSTRAINT result_snapshot_schema_version_positive_check CHECK (
            requested_output_schema_version > 0
        ),
    narrative_version_ref TEXT CONSTRAINT result_snapshot_narrative_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_narrative_ref_format_check CHECK (
            narrative_version_ref = btrim(narrative_version_ref)
            AND narrative_version_ref <> ''
            AND NOT (
                narrative_version_ref ~ '[[:digit:]]'
                AND narrative_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consent_snapshot_refs TEXT[] CONSTRAINT result_snapshot_consent_refs_not_null NOT NULL
        CONSTRAINT result_snapshot_consent_refs_not_empty_check CHECK (
            cardinality(consent_snapshot_refs) > 0
        )
        CONSTRAINT result_snapshot_consent_refs_integrity_check CHECK (
            result_snapshot_consent_refs_are_valid(consent_snapshot_refs)
        ),
    engine_artifact_digest TEXT CONSTRAINT result_snapshot_engine_digest_not_null NOT NULL
        CONSTRAINT result_snapshot_engine_digest_format_check CHECK (
            engine_artifact_digest = btrim(engine_artifact_digest)
            AND engine_artifact_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    created_at_unix_ms BIGINT CONSTRAINT result_snapshot_created_at_unix_not_null NOT NULL
        CONSTRAINT result_snapshot_created_at_unix_positive_check CHECK (created_at_unix_ms > 0),
    supersedes_ref TEXT
        CONSTRAINT result_snapshot_supersedes_ref_format_check CHECK (
            supersedes_ref IS NULL OR (
                supersedes_ref = btrim(supersedes_ref)
                AND supersedes_ref <> ''
                AND supersedes_ref <> result_snapshot_ref
                AND NOT (
                    supersedes_ref ~ '[[:digit:]]'
                    AND supersedes_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    created_at TIMESTAMPTZ CONSTRAINT result_snapshot_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT result_snapshot_pkey PRIMARY KEY (result_snapshot_ref)
);

CREATE TABLE IF NOT EXISTS result_snapshot_observation (
    result_snapshot_ref TEXT CONSTRAINT result_snapshot_observation_snapshot_ref_not_null NOT NULL,
    observation_order INTEGER
        CONSTRAINT result_snapshot_observation_order_not_null NOT NULL
        CONSTRAINT result_snapshot_observation_order_nonnegative_check CHECK (
            observation_order >= 0
        ),
    construct_ref TEXT CONSTRAINT result_snapshot_observation_construct_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_observation_construct_ref_format_check CHECK (
            construct_ref = btrim(construct_ref)
            AND construct_ref <> ''
            AND NOT (
                construct_ref ~ '[[:digit:]]'
                AND construct_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    observation_disposition TEXT
        CONSTRAINT result_snapshot_observation_disposition_not_null NOT NULL
        CONSTRAINT result_snapshot_observation_disposition_value_check CHECK (
            observation_disposition IN ('scored', 'abstained', 'failed', 'excluded')
        ),
    score DOUBLE PRECISION
        CONSTRAINT result_snapshot_observation_score_finite_check CHECK (
            score IS NULL OR (
                score > '-Infinity'::double precision
                AND score < 'Infinity'::double precision
            )
        ),
    standard_error DOUBLE PRECISION
        CONSTRAINT result_snapshot_observation_standard_error_shape_check CHECK (
            standard_error IS NULL OR (
                standard_error >= 0
                AND standard_error < 'Infinity'::double precision
            )
        ),
    CONSTRAINT result_snapshot_observation_pkey PRIMARY KEY (
        result_snapshot_ref,
        construct_ref
    ),
    CONSTRAINT result_snapshot_observation_order_unique UNIQUE (
        result_snapshot_ref,
        observation_order
    ),
    CONSTRAINT result_snapshot_observation_snapshot_fk FOREIGN KEY (result_snapshot_ref)
        REFERENCES result_snapshot (result_snapshot_ref),
    CONSTRAINT result_snapshot_observation_score_shape_check CHECK (
        (
            observation_disposition = 'scored'
            AND score IS NOT NULL
        )
        OR (
            observation_disposition <> 'scored'
            AND score IS NULL
            AND standard_error IS NULL
        )
    )
);

-- Reapplying this migration must also strengthen a schema created by an earlier
-- revision of this not-yet-released migration. PostgreSQL's CREATE TABLE IF NOT
-- EXISTS does not reconcile changed CHECK definitions on an existing table.
ALTER TABLE result_snapshot
    DROP CONSTRAINT IF EXISTS result_snapshot_consent_refs_integrity_check;
ALTER TABLE result_snapshot
    ADD CONSTRAINT result_snapshot_consent_refs_integrity_check CHECK (
        result_snapshot_consent_refs_are_valid(consent_snapshot_refs)
    );
ALTER TABLE result_snapshot
    DROP CONSTRAINT IF EXISTS result_snapshot_engine_digest_format_check;
ALTER TABLE result_snapshot
    ADD CONSTRAINT result_snapshot_engine_digest_format_check CHECK (
        engine_artifact_digest = btrim(engine_artifact_digest)
        AND engine_artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    );
ALTER TABLE result_snapshot_observation
    DROP CONSTRAINT IF EXISTS result_snapshot_observation_score_finite_check;
ALTER TABLE result_snapshot_observation
    ADD CONSTRAINT result_snapshot_observation_score_finite_check CHECK (
        score IS NULL OR (
            score > '-Infinity'::double precision
            AND score < 'Infinity'::double precision
        )
    );
ALTER TABLE result_snapshot_observation
    DROP CONSTRAINT IF EXISTS result_snapshot_observation_standard_error_shape_check;
ALTER TABLE result_snapshot_observation
    ADD CONSTRAINT result_snapshot_observation_standard_error_shape_check CHECK (
        standard_error IS NULL OR (
            standard_error >= 0
            AND standard_error < 'Infinity'::double precision
        )
    );

CREATE OR REPLACE FUNCTION reject_result_snapshot_evidence_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'result snapshot evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS result_snapshot_immutable_guard
    ON result_snapshot;
CREATE TRIGGER result_snapshot_immutable_guard
    BEFORE UPDATE OR DELETE ON result_snapshot
    FOR EACH ROW
    EXECUTE FUNCTION reject_result_snapshot_evidence_mutation();

DROP TRIGGER IF EXISTS result_snapshot_truncate_guard
    ON result_snapshot;
CREATE TRIGGER result_snapshot_truncate_guard
    BEFORE TRUNCATE ON result_snapshot
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_result_snapshot_evidence_mutation();

DROP TRIGGER IF EXISTS result_snapshot_observation_immutable_guard
    ON result_snapshot_observation;
CREATE TRIGGER result_snapshot_observation_immutable_guard
    BEFORE UPDATE OR DELETE ON result_snapshot_observation
    FOR EACH ROW
    EXECUTE FUNCTION reject_result_snapshot_evidence_mutation();

DROP TRIGGER IF EXISTS result_snapshot_observation_truncate_guard
    ON result_snapshot_observation;
CREATE TRIGGER result_snapshot_observation_truncate_guard
    BEFORE TRUNCATE ON result_snapshot_observation
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_result_snapshot_evidence_mutation();
