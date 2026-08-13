CREATE TABLE IF NOT EXISTS instrument_release (
    release_ref TEXT CONSTRAINT instrument_release_release_ref_not_null NOT NULL
        CONSTRAINT instrument_release_release_ref_format_check CHECK (
            release_ref = btrim(release_ref)
            AND release_ref <> ''
            AND NOT (
                release_ref ~ '[[:digit:]]'
                AND release_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_ref TEXT CONSTRAINT instrument_release_instrument_ref_not_null NOT NULL
        CONSTRAINT instrument_release_instrument_ref_format_check CHECK (
            instrument_ref = btrim(instrument_ref)
            AND instrument_ref <> ''
            AND NOT (
                instrument_ref ~ '[[:digit:]]'
                AND instrument_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    instrument_version_ref TEXT CONSTRAINT instrument_release_version_ref_not_null NOT NULL
        CONSTRAINT instrument_release_version_ref_format_check CHECK (
            instrument_version_ref = btrim(instrument_version_ref)
            AND instrument_version_ref <> ''
            AND NOT (
                instrument_version_ref ~ '[[:digit:]]'
                AND instrument_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    construct_ref TEXT CONSTRAINT instrument_release_construct_ref_not_null NOT NULL
        CONSTRAINT instrument_release_construct_ref_format_check CHECK (
            construct_ref = btrim(construct_ref)
            AND construct_ref <> ''
            AND NOT (
                construct_ref ~ '[[:digit:]]'
                AND construct_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    item_version_refs TEXT[] CONSTRAINT instrument_release_item_refs_not_null NOT NULL
        CONSTRAINT instrument_release_item_refs_not_empty_check CHECK (
            cardinality(item_version_refs) > 0
        ),
    locale TEXT CONSTRAINT instrument_release_locale_not_null NOT NULL
        CONSTRAINT instrument_release_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    assessment_spec_ref TEXT CONSTRAINT instrument_release_spec_ref_not_null NOT NULL
        CONSTRAINT instrument_release_spec_ref_format_check CHECK (
            assessment_spec_ref = btrim(assessment_spec_ref)
            AND assessment_spec_ref <> ''
            AND NOT (
                assessment_spec_ref ~ '[[:digit:]]'
                AND assessment_spec_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scoring_version_ref TEXT CONSTRAINT instrument_release_scoring_ref_not_null NOT NULL
        CONSTRAINT instrument_release_scoring_ref_format_check CHECK (
            scoring_version_ref = btrim(scoring_version_ref)
            AND scoring_version_ref <> ''
            AND NOT (
                scoring_version_ref ~ '[[:digit:]]'
                AND scoring_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    calibration_reference TEXT CONSTRAINT instrument_release_calibration_ref_not_null NOT NULL
        CONSTRAINT instrument_release_calibration_ref_format_check CHECK (
            calibration_reference = btrim(calibration_reference)
            AND calibration_reference <> ''
            AND NOT (
                calibration_reference ~ '[[:digit:]]'
                AND calibration_reference ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    norm_version_ref TEXT
        CONSTRAINT instrument_release_norm_ref_format_check CHECK (
            norm_version_ref IS NULL OR (
                norm_version_ref = btrim(norm_version_ref)
                AND norm_version_ref <> ''
                AND NOT (
                    norm_version_ref ~ '[[:digit:]]'
                    AND norm_version_ref ~ '^[[:digit:]+,.eE-]+$'
                )
            )
        ),
    narrative_version_ref TEXT CONSTRAINT instrument_release_narrative_ref_not_null NOT NULL
        CONSTRAINT instrument_release_narrative_ref_format_check CHECK (
            narrative_version_ref = btrim(narrative_version_ref)
            AND narrative_version_ref <> ''
            AND NOT (
                narrative_version_ref ~ '[[:digit:]]'
                AND narrative_version_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    consent_requirement_refs TEXT[] CONSTRAINT instrument_release_consent_refs_not_null NOT NULL
        CONSTRAINT instrument_release_consent_refs_not_empty_check CHECK (
            cardinality(consent_requirement_refs) > 0
        ),
    intended_use_ref TEXT CONSTRAINT instrument_release_intended_use_ref_not_null NOT NULL
        CONSTRAINT instrument_release_intended_use_ref_format_check CHECK (
            intended_use_ref = btrim(intended_use_ref)
            AND intended_use_ref <> ''
            AND NOT (
                intended_use_ref ~ '[[:digit:]]'
                AND intended_use_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    limitations_ref TEXT CONSTRAINT instrument_release_limitations_ref_not_null NOT NULL
        CONSTRAINT instrument_release_limitations_ref_format_check CHECK (
            limitations_ref = btrim(limitations_ref)
            AND limitations_ref <> ''
            AND NOT (
                limitations_ref ~ '[[:digit:]]'
                AND limitations_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    content_digest TEXT CONSTRAINT instrument_release_digest_not_null NOT NULL
        CONSTRAINT instrument_release_digest_format_check CHECK (
            content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    publication_state TEXT CONSTRAINT instrument_release_state_not_null NOT NULL
        CONSTRAINT instrument_release_state_value_check CHECK (
            publication_state IN ('draft', 'review', 'published', 'suspended', 'retired')
        ),
    created_at_unix_ms BIGINT CONSTRAINT instrument_release_created_at_unix_not_null NOT NULL
        CONSTRAINT instrument_release_created_at_unix_positive_check CHECK (created_at_unix_ms > 0),
    created_at TIMESTAMPTZ CONSTRAINT instrument_release_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT instrument_release_pkey PRIMARY KEY (release_ref)
);
