-- Opaque item-delivery references deliberately remain data, not SQL syntax. The
-- persistence adapter binds them as query parameters. These helpers enforce the
-- canonical identity boundary (nonblank, nonnumeric-like, no outer whitespace).
--
-- PostgreSQL assumes functions used by CHECK constraints are immutable. A future
-- migration that changes either predicate MUST recreate every dependent CHECK
-- constraint and run ALTER TABLE ... VALIDATE CONSTRAINT so pre-existing rows are
-- re-evaluated under the new predicate before the migration is considered complete.
CREATE OR REPLACE FUNCTION item_delivery_reference_is_valid(reference_value TEXT)
RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $item_delivery_reference$
    SELECT
        reference_value IS NOT NULL
        AND reference_value <> ''
        AND left(reference_value, 1) !~ '[[:space:]]'
        AND right(reference_value, 1) !~ '[[:space:]]'
        AND NOT (
            reference_value ~ '[[:digit:]]'
            AND reference_value ~ '^[[:digit:]+,.eE-]+$'
        );
$item_delivery_reference$;

CREATE OR REPLACE FUNCTION item_delivery_reference_array_is_valid(reference_values TEXT[])
RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $item_delivery_reference_array$
    SELECT
        COALESCE(
            bool_and(
                reference_value IS NOT NULL
                AND reference_value <> ''
                AND left(reference_value, 1) !~ '[[:space:]]'
                AND right(reference_value, 1) !~ '[[:space:]]'
                AND NOT (
                    reference_value ~ '[[:digit:]]'
                    AND reference_value ~ '^[[:digit:]+,.eE-]+$'
                )
            ),
            TRUE
        )
        AND COUNT(*) = COUNT(DISTINCT reference_value)
    FROM unnest(reference_values) AS allowed_reference(reference_value);
$item_delivery_reference_array$;

CREATE TABLE IF NOT EXISTS item_delivery_ledger (
    tenant_ref TEXT CONSTRAINT item_delivery_ledger_tenant_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        ),
    session_ref TEXT CONSTRAINT item_delivery_ledger_session_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_session_ref_format_check CHECK (
            item_delivery_reference_is_valid(session_ref)
        ),
    instrument_release_ref TEXT CONSTRAINT item_delivery_ledger_release_ref_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_release_ref_format_check CHECK (
            item_delivery_reference_is_valid(instrument_release_ref)
        ),
    release_content_digest TEXT CONSTRAINT item_delivery_ledger_digest_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_digest_format_check CHECK (
            release_content_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    locale TEXT CONSTRAINT item_delivery_ledger_locale_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_locale_format_check CHECK (
            locale = btrim(locale)
            AND locale ~ '^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$'
        ),
    allowed_item_version_refs TEXT[] CONSTRAINT item_delivery_ledger_allowed_items_not_null NOT NULL
        CONSTRAINT item_delivery_ledger_allowed_items_not_empty_check CHECK (
            cardinality(allowed_item_version_refs) > 0
        )
        CONSTRAINT item_delivery_ledger_allowed_items_format_check CHECK (
            item_delivery_reference_array_is_valid(allowed_item_version_refs)
        ),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_ledger_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_ledger_pkey PRIMARY KEY (session_ref),
    CONSTRAINT item_delivery_ledger_tenant_session_unique UNIQUE (tenant_ref, session_ref)
);

CREATE TABLE IF NOT EXISTS item_delivery_event (
    tenant_ref TEXT CONSTRAINT item_delivery_event_tenant_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_tenant_ref_format_check CHECK (
            item_delivery_reference_is_valid(tenant_ref)
        ),
    session_ref TEXT CONSTRAINT item_delivery_event_session_ref_not_null NOT NULL,
    delivery_event_ref TEXT CONSTRAINT item_delivery_event_delivery_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_delivery_ref_format_check CHECK (
            item_delivery_reference_is_valid(delivery_event_ref)
        ),
    item_version_ref TEXT CONSTRAINT item_delivery_event_item_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_item_ref_format_check CHECK (
            item_delivery_reference_is_valid(item_version_ref)
        ),
    presentation_context_ref TEXT CONSTRAINT item_delivery_event_presentation_ref_not_null NOT NULL
        CONSTRAINT item_delivery_event_presentation_ref_format_check CHECK (
            item_delivery_reference_is_valid(presentation_context_ref)
        ),
    selection_evidence_ref TEXT
        CONSTRAINT item_delivery_event_selection_ref_format_check CHECK (
            selection_evidence_ref IS NULL
            OR item_delivery_reference_is_valid(selection_evidence_ref)
        ),
    delivery_sequence BIGINT CONSTRAINT item_delivery_event_sequence_not_null NOT NULL
        CONSTRAINT item_delivery_event_sequence_positive_check CHECK (delivery_sequence > 0),
    created_at TIMESTAMPTZ CONSTRAINT item_delivery_event_created_at_not_null NOT NULL
        DEFAULT clock_timestamp(),
    CONSTRAINT item_delivery_event_pkey PRIMARY KEY (session_ref, delivery_event_ref),
    CONSTRAINT item_delivery_event_delivery_ref_unique UNIQUE (delivery_event_ref),
    CONSTRAINT item_delivery_event_session_tenant_fk FOREIGN KEY (tenant_ref, session_ref)
        REFERENCES item_delivery_ledger (tenant_ref, session_ref),
    CONSTRAINT item_delivery_event_item_version_unique UNIQUE (session_ref, item_version_ref),
    CONSTRAINT item_delivery_event_sequence_unique UNIQUE (session_ref, delivery_sequence)
);