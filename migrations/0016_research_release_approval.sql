CREATE TABLE IF NOT EXISTS research_release_approval (
    research_release_ref TEXT CONSTRAINT research_release_approval_release_ref_not_null NOT NULL
        CONSTRAINT research_release_approval_release_ref_format_check CHECK (
            research_release_ref = btrim(research_release_ref)
            AND research_release_ref <> ''
            AND NOT (
                research_release_ref ~ '[[:digit:]]'
                AND research_release_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    dataset_snapshot_ref TEXT CONSTRAINT research_release_approval_dataset_ref_not_null NOT NULL
        CONSTRAINT research_release_approval_dataset_ref_format_check CHECK (
            dataset_snapshot_ref = btrim(dataset_snapshot_ref)
            AND dataset_snapshot_ref <> ''
            AND NOT (
                dataset_snapshot_ref ~ '[[:digit:]]'
                AND dataset_snapshot_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    research_scope_ref TEXT CONSTRAINT research_release_approval_scope_ref_not_null NOT NULL
        CONSTRAINT research_release_approval_scope_ref_format_check CHECK (
            research_scope_ref = btrim(research_scope_ref)
            AND research_scope_ref <> ''
            AND NOT (
                research_scope_ref ~ '[[:digit:]]'
                AND research_scope_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    manifest_digest TEXT CONSTRAINT research_release_approval_manifest_digest_not_null NOT NULL
        CONSTRAINT research_release_approval_manifest_digest_format_check CHECK (
            manifest_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    privacy_review_ref TEXT CONSTRAINT research_release_approval_privacy_review_not_null NOT NULL
        CONSTRAINT research_release_approval_privacy_review_format_check CHECK (
            privacy_review_ref = btrim(privacy_review_ref)
            AND privacy_review_ref <> ''
            AND NOT (
                privacy_review_ref ~ '[[:digit:]]'
                AND privacy_review_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    scientific_review_ref TEXT CONSTRAINT research_release_approval_scientific_review_not_null NOT NULL
        CONSTRAINT research_release_approval_scientific_review_format_check CHECK (
            scientific_review_ref = btrim(scientific_review_ref)
            AND scientific_review_ref <> ''
            AND NOT (
                scientific_review_ref ~ '[[:digit:]]'
                AND scientific_review_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    metadata_bundle_ref TEXT CONSTRAINT research_release_approval_metadata_bundle_not_null NOT NULL
        CONSTRAINT research_release_approval_metadata_bundle_format_check CHECK (
            metadata_bundle_ref = btrim(metadata_bundle_ref)
            AND metadata_bundle_ref <> ''
            AND NOT (
                metadata_bundle_ref ~ '[[:digit:]]'
                AND metadata_bundle_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    license_record_ref TEXT CONSTRAINT research_release_approval_license_record_not_null NOT NULL
        CONSTRAINT research_release_approval_license_record_format_check CHECK (
            license_record_ref = btrim(license_record_ref)
            AND license_record_ref <> ''
            AND NOT (
                license_record_ref ~ '[[:digit:]]'
                AND license_record_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    measurement_provenance_ref TEXT CONSTRAINT research_release_approval_measurement_provenance_not_null NOT NULL
        CONSTRAINT research_release_approval_measurement_provenance_format_check CHECK (
            measurement_provenance_ref = btrim(measurement_provenance_ref)
            AND measurement_provenance_ref <> ''
            AND NOT (
                measurement_provenance_ref ~ '[[:digit:]]'
                AND measurement_provenance_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    access_approval_ref TEXT CONSTRAINT research_release_approval_access_approval_not_null NOT NULL
        CONSTRAINT research_release_approval_access_approval_format_check CHECK (
            access_approval_ref = btrim(access_approval_ref)
            AND access_approval_ref <> ''
            AND NOT (
                access_approval_ref ~ '[[:digit:]]'
                AND access_approval_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    citation_metadata_ref TEXT CONSTRAINT research_release_approval_citation_metadata_not_null NOT NULL
        CONSTRAINT research_release_approval_citation_metadata_format_check CHECK (
            citation_metadata_ref = btrim(citation_metadata_ref)
            AND citation_metadata_ref <> ''
            AND NOT (
                citation_metadata_ref ~ '[[:digit:]]'
                AND citation_metadata_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    release_approver_ref TEXT CONSTRAINT research_release_approval_release_approver_not_null NOT NULL
        CONSTRAINT research_release_approval_release_approver_format_check CHECK (
            release_approver_ref = btrim(release_approver_ref)
            AND release_approver_ref <> ''
            AND NOT (
                release_approver_ref ~ '[[:digit:]]'
                AND release_approver_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    ordinary_admin_ref TEXT CONSTRAINT research_release_approval_ordinary_admin_not_null NOT NULL
        CONSTRAINT research_release_approval_ordinary_admin_format_check CHECK (
            ordinary_admin_ref = btrim(ordinary_admin_ref)
            AND ordinary_admin_ref <> ''
            AND NOT (
                ordinary_admin_ref ~ '[[:digit:]]'
                AND ordinary_admin_ref ~ '^[[:digit:]+,.eE-]+$'
            )
        ),
    access_class TEXT CONSTRAINT research_release_approval_access_class_not_null NOT NULL
        CONSTRAINT research_release_approval_access_class_value_check CHECK (
            access_class IN ('public', 'controlled', 'private', 'embargoed')
        ),
    created_at TIMESTAMPTZ CONSTRAINT research_release_approval_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT research_release_approval_pkey PRIMARY KEY (research_release_ref),
    CONSTRAINT research_release_approval_separation_of_duties_check CHECK (
        release_approver_ref <> ordinary_admin_ref
    )
);

CREATE OR REPLACE FUNCTION reject_research_release_approval_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'research_release_approval rows are immutable'
        USING ERRCODE = '55000';
END;
$$;

DROP TRIGGER IF EXISTS research_release_approval_immutable_guard
    ON research_release_approval;
CREATE TRIGGER research_release_approval_immutable_guard
    BEFORE UPDATE OR DELETE ON research_release_approval
    FOR EACH ROW
    EXECUTE FUNCTION reject_research_release_approval_mutation();

DROP TRIGGER IF EXISTS research_release_approval_truncate_guard
    ON research_release_approval;
CREATE TRIGGER research_release_approval_truncate_guard
    BEFORE TRUNCATE ON research_release_approval
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_research_release_approval_mutation();
