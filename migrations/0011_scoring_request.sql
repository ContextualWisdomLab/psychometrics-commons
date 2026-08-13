CREATE TABLE IF NOT EXISTS scoring_request (
    scoring_request_ref TEXT CONSTRAINT scoring_request_scoring_request_ref_not_null NOT NULL
        CONSTRAINT scoring_request_scoring_request_ref_format_check CHECK (
            scoring_request_ref = btrim(scoring_request_ref)
            AND scoring_request_ref <> ''
            AND NOT (
                scoring_request_ref ~ '[[:digit:]]'
                AND scoring_request_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT scoring_request_session_ref_not_null NOT NULL
        CONSTRAINT scoring_request_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    response_snapshot_ref TEXT CONSTRAINT scoring_request_response_snapshot_ref_not_null NOT NULL
        CONSTRAINT scoring_request_response_snapshot_ref_format_check CHECK (
            response_snapshot_ref = btrim(response_snapshot_ref)
            AND response_snapshot_ref <> ''
            AND NOT (
                response_snapshot_ref ~ '[[:digit:]]'
                AND response_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    assessment_spec_ref TEXT CONSTRAINT scoring_request_assessment_spec_ref_not_null NOT NULL
        CONSTRAINT scoring_request_assessment_spec_ref_format_check CHECK (
            assessment_spec_ref = btrim(assessment_spec_ref)
            AND assessment_spec_ref <> ''
            AND NOT (
                assessment_spec_ref ~ '[[:digit:]]'
                AND assessment_spec_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_version_ref TEXT CONSTRAINT scoring_request_instrument_version_ref_not_null NOT NULL
        CONSTRAINT scoring_request_instrument_version_ref_format_check CHECK (
            instrument_version_ref = btrim(instrument_version_ref)
            AND instrument_version_ref <> ''
            AND NOT (
                instrument_version_ref ~ '[[:digit:]]'
                AND instrument_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_version_ref TEXT CONSTRAINT scoring_request_scoring_version_ref_not_null NOT NULL
        CONSTRAINT scoring_request_scoring_version_ref_format_check CHECK (
            scoring_version_ref = btrim(scoring_version_ref)
            AND scoring_version_ref <> ''
            AND NOT (
                scoring_version_ref ~ '[[:digit:]]'
                AND scoring_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    calibration_reference TEXT CONSTRAINT scoring_request_calibration_reference_not_null NOT NULL
        CONSTRAINT scoring_request_calibration_reference_format_check CHECK (
            calibration_reference = btrim(calibration_reference)
            AND calibration_reference <> ''
            AND NOT (
                calibration_reference ~ '[[:digit:]]'
                AND calibration_reference ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    norm_version_ref TEXT
        CONSTRAINT scoring_request_norm_version_ref_format_check CHECK (
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
        CONSTRAINT scoring_request_schema_version_not_null NOT NULL
        CONSTRAINT scoring_request_schema_version_positive_check CHECK (
            requested_output_schema_version > 0
        ),
    created_at TIMESTAMPTZ CONSTRAINT scoring_request_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT scoring_request_pkey PRIMARY KEY (scoring_request_ref)
);
