-- Immutable result provenance uses the same opaque-reference boundary as the Rust domain.
-- Rust 1.97 `char::is_numeric` includes Unicode Nd/No/Nl characters that PostgreSQL's POSIX
-- digit class does not cover. PostgreSQL 18 UTF-8 with pg_unicode_fast supplies matching
-- whitespace/control classification; the generated int4multirange is Rust 1.97 Unicode 17.
CREATE OR REPLACE FUNCTION result_snapshot_reference_is_valid(reference_text TEXT)
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $result_snapshot_reference$
    WITH reference_character AS (
        SELECT substr(reference_text, character_index, 1) AS character_text
        FROM generate_series(1, character_length(reference_text)) AS character_index
    ),
    reference_classification AS (
        SELECT
            character_text,
            ascii(character_text) <@ '{[48,58),[178,180),[185,186),[188,191),[1632,1642),[1776,1786),[1984,1994),[2406,2416),[2534,2544),[2548,2554),[2662,2672),[2790,2800),[2918,2928),[2930,2936),[3046,3059),[3174,3184),[3192,3199),[3302,3312),[3416,3423),[3430,3449),[3558,3568),[3664,3674),[3792,3802),[3872,3892),[4160,4170),[4240,4250),[4969,4989),[5870,5873),[6112,6122),[6128,6138),[6160,6170),[6470,6480),[6608,6619),[6784,6794),[6800,6810),[6992,7002),[7088,7098),[7232,7242),[7248,7258),[8304,8305),[8308,8314),[8320,8330),[8528,8579),[8581,8586),[9312,9372),[9450,9472),[10102,10132),[11517,11518),[12295,12296),[12321,12330),[12344,12347),[12690,12694),[12832,12842),[12872,12880),[12881,12896),[12928,12938),[12977,12992),[42528,42538),[42726,42736),[43056,43062),[43216,43226),[43264,43274),[43472,43482),[43504,43514),[43600,43610),[44016,44026),[65296,65306),[65799,65844),[65856,65913),[65930,65932),[66273,66300),[66336,66340),[66369,66370),[66378,66379),[66513,66518),[66720,66730),[67672,67680),[67705,67712),[67751,67760),[67835,67840),[67862,67868),[68028,68030),[68032,68048),[68050,68096),[68160,68169),[68221,68223),[68253,68256),[68331,68336),[68440,68448),[68472,68480),[68521,68528),[68858,68864),[68912,68922),[68928,68938),[69216,69247),[69405,69415),[69457,69461),[69573,69580),[69714,69744),[69872,69882),[69942,69952),[70096,70106),[70113,70133),[70384,70394),[70736,70746),[70864,70874),[71248,71258),[71360,71370),[71376,71396),[71472,71484),[71904,71923),[72016,72026),[72688,72698),[72784,72813),[73040,73050),[73120,73130),[73184,73194),[73552,73562),[73664,73685),[74752,74863),[90416,90426),[92768,92778),[92864,92874),[93008,93018),[93019,93026),[93552,93562),[93824,93847),[94196,94199),[118000,118010),[119488,119508),[119520,119540),[119648,119673),[120782,120832),[123200,123210),[123632,123642),[124144,124154),[124401,124411),[125127,125136),[125264,125274),[126065,126124),[126125,126128),[126129,126133),[126209,126254),[126255,126270),[127232,127245),[130032,130042)}'::int4multirange AS is_numeric
        FROM reference_character
    )
    SELECT
        reference_text <> ''
        AND reference_text COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND reference_text COLLATE "pg_unicode_fast" !~ '[[:cntrl:]]'
        AND NOT COALESCE(
            bool_or(is_numeric)
            AND bool_and(
                is_numeric
                OR character_text = ANY (
                    ARRAY['+', '-', '.', ',', 'e', 'E', U&'\066B', U&'\066C', U&'\FF0E', U&'\FF0C']
                )
            ),
            FALSE
        )
    FROM reference_classification;
$result_snapshot_reference$;

CREATE OR REPLACE FUNCTION result_snapshot_consent_refs_are_valid(reference_array TEXT[])
RETURNS BOOLEAN
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
-- Capture the migration's current schema so this array validator delegates to
-- the scalar validator created beside it without hard-coding `public`.
SET search_path FROM CURRENT
AS $result_snapshot_consent_refs$
    WITH consent_reference AS (
        SELECT reference_text
        FROM unnest(reference_array) AS consent_reference(reference_text)
    )
    SELECT
        count(*) = count(DISTINCT reference_text)
        AND COALESCE(
            bool_and(
                reference_text IS NOT NULL
                AND result_snapshot_reference_is_valid(reference_text)
            ),
            TRUE
        )
    FROM consent_reference;
$result_snapshot_consent_refs$;

CREATE TABLE IF NOT EXISTS result_snapshot (
    result_snapshot_ref TEXT CONSTRAINT result_snapshot_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_ref_format_check CHECK (
            result_snapshot_reference_is_valid(result_snapshot_ref)
        ),
    participant_ref TEXT CONSTRAINT result_snapshot_participant_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_participant_ref_format_check CHECK (
            result_snapshot_reference_is_valid(participant_ref)
        ),
    scoring_result_ref TEXT CONSTRAINT result_snapshot_scoring_result_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_scoring_result_ref_format_check CHECK (
            result_snapshot_reference_is_valid(scoring_result_ref)
        ),
    session_ref TEXT CONSTRAINT result_snapshot_session_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_session_ref_format_check CHECK (
            result_snapshot_reference_is_valid(session_ref)
        ),
    response_snapshot_ref TEXT CONSTRAINT result_snapshot_response_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_response_ref_format_check CHECK (
            result_snapshot_reference_is_valid(response_snapshot_ref)
        ),
    assessment_spec_ref TEXT CONSTRAINT result_snapshot_spec_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_spec_ref_format_check CHECK (
            result_snapshot_reference_is_valid(assessment_spec_ref)
        ),
    instrument_version_ref TEXT CONSTRAINT result_snapshot_instrument_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_instrument_ref_format_check CHECK (
            result_snapshot_reference_is_valid(instrument_version_ref)
        ),
    scoring_version_ref TEXT CONSTRAINT result_snapshot_scoring_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_scoring_ref_format_check CHECK (
            result_snapshot_reference_is_valid(scoring_version_ref)
        ),
    calibration_reference TEXT CONSTRAINT result_snapshot_calibration_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_calibration_ref_format_check CHECK (
            result_snapshot_reference_is_valid(calibration_reference)
        ),
    norm_version_ref TEXT
        CONSTRAINT result_snapshot_norm_ref_format_check CHECK (
            norm_version_ref IS NULL OR result_snapshot_reference_is_valid(norm_version_ref)
        ),
    requested_output_schema_version INTEGER
        CONSTRAINT result_snapshot_schema_version_not_null NOT NULL
        CONSTRAINT result_snapshot_schema_version_positive_check CHECK (
            requested_output_schema_version > 0
        ),
    narrative_version_ref TEXT CONSTRAINT result_snapshot_narrative_ref_not_null NOT NULL
        CONSTRAINT result_snapshot_narrative_ref_format_check CHECK (
            result_snapshot_reference_is_valid(narrative_version_ref)
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
                supersedes_ref <> result_snapshot_ref
                AND result_snapshot_reference_is_valid(supersedes_ref)
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
            result_snapshot_reference_is_valid(construct_ref)
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

-- Reapplying this migration must strengthen historical CHECK definitions as well as fresh tables.
-- PostgreSQL's CREATE TABLE IF NOT EXISTS does not reconcile changed constraints on an existing
-- schema, so every reference predicate is dropped and recreated, forcing existing rows through
-- the Rust-equivalent validator before the migration can succeed.
ALTER TABLE result_snapshot_observation DROP CONSTRAINT IF EXISTS result_snapshot_observation_construct_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_supersedes_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_consent_refs_integrity_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_narrative_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_norm_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_calibration_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_scoring_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_instrument_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_spec_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_response_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_session_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_scoring_result_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_participant_ref_format_check;
ALTER TABLE result_snapshot DROP CONSTRAINT IF EXISTS result_snapshot_ref_format_check;

ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_ref_format_check CHECK (
    result_snapshot_reference_is_valid(result_snapshot_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_participant_ref_format_check CHECK (
    result_snapshot_reference_is_valid(participant_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_scoring_result_ref_format_check CHECK (
    result_snapshot_reference_is_valid(scoring_result_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_session_ref_format_check CHECK (
    result_snapshot_reference_is_valid(session_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_response_ref_format_check CHECK (
    result_snapshot_reference_is_valid(response_snapshot_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_spec_ref_format_check CHECK (
    result_snapshot_reference_is_valid(assessment_spec_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_instrument_ref_format_check CHECK (
    result_snapshot_reference_is_valid(instrument_version_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_scoring_ref_format_check CHECK (
    result_snapshot_reference_is_valid(scoring_version_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_calibration_ref_format_check CHECK (
    result_snapshot_reference_is_valid(calibration_reference)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_norm_ref_format_check CHECK (
    norm_version_ref IS NULL OR result_snapshot_reference_is_valid(norm_version_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_narrative_ref_format_check CHECK (
    result_snapshot_reference_is_valid(narrative_version_ref)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_consent_refs_integrity_check CHECK (
    result_snapshot_consent_refs_are_valid(consent_snapshot_refs)
);
ALTER TABLE result_snapshot ADD CONSTRAINT result_snapshot_supersedes_ref_format_check CHECK (
    supersedes_ref IS NULL OR (
        supersedes_ref <> result_snapshot_ref
        AND result_snapshot_reference_is_valid(supersedes_ref)
    )
);
ALTER TABLE result_snapshot_observation ADD CONSTRAINT result_snapshot_observation_construct_ref_format_check CHECK (
    result_snapshot_reference_is_valid(construct_ref)
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

-- Supersession is an immutable backward link. Reapplying the migration must fail
-- rather than preserving dangling or cyclic lineage written by an older schema.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM result_snapshot AS successor
        LEFT JOIN result_snapshot AS predecessor
          ON predecessor.result_snapshot_ref = successor.supersedes_ref
        WHERE successor.supersedes_ref IS NOT NULL
          AND predecessor.result_snapshot_ref IS NULL
    ) THEN
        RAISE EXCEPTION 'result snapshot supersession predecessor must already exist'
            USING ERRCODE = '23503';
    END IF;

    IF EXISTS (
        WITH RECURSIVE supersession_lineage AS (
            SELECT
                result_snapshot_ref AS start_ref,
                supersedes_ref AS current_ref,
                ARRAY[result_snapshot_ref]::text[] AS visited_refs
            FROM result_snapshot
            WHERE supersedes_ref IS NOT NULL

            UNION ALL

            SELECT
                lineage.start_ref,
                predecessor.supersedes_ref,
                lineage.visited_refs || predecessor.result_snapshot_ref
            FROM supersession_lineage AS lineage
            JOIN result_snapshot AS predecessor
              ON predecessor.result_snapshot_ref = lineage.current_ref
            WHERE lineage.current_ref IS NOT NULL
              AND NOT predecessor.result_snapshot_ref = ANY(lineage.visited_refs)
        )
        SELECT 1
        FROM supersession_lineage
        WHERE current_ref IS NOT NULL
          AND current_ref = ANY(visited_refs)
    ) THEN
        RAISE EXCEPTION 'result snapshot supersession lineage must be acyclic'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION require_result_snapshot_supersession_predecessor()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- Preserve the named table CHECK as the authoritative classifier for
    -- self-supersession. The predecessor trigger only owns references to a
    -- different row; otherwise a self-reference is misreported as a missing
    -- predecessor before PostgreSQL can evaluate the CHECK constraint.
    IF NEW.supersedes_ref IS NULL
       OR NEW.supersedes_ref = NEW.result_snapshot_ref THEN
        RETURN NEW;
    END IF;

    PERFORM 1
    FROM result_snapshot
    WHERE result_snapshot_ref = NEW.supersedes_ref;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'result snapshot supersession predecessor must already exist'
            USING ERRCODE = '23503';
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS result_snapshot_supersession_predecessor_guard
    ON result_snapshot;
CREATE TRIGGER result_snapshot_supersession_predecessor_guard
    BEFORE INSERT ON result_snapshot
    FOR EACH ROW
    EXECUTE FUNCTION require_result_snapshot_supersession_predecessor();

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