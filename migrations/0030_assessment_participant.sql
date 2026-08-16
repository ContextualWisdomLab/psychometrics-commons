-- Durable product-owned anonymous-first participant identity.
--
-- This table stores only the stable Psychometrics Commons participant base record.
-- Optional Keyverse link history remains a separate append-only identity-link concern.
-- PostgreSQL 18's pg_unicode_fast collation gives the reference guards stable Unicode
-- whitespace and decimal-digit classification instead of inheriting host LC_CTYPE behavior.

CREATE TABLE IF NOT EXISTS assessment_participant (
    participant_ref TEXT PRIMARY KEY,
    tenant_ref TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL,

    CONSTRAINT assessment_participant_ref_format_check CHECK (
        participant_ref <> ''
        AND participant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            participant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND participant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT assessment_participant_tenant_ref_format_check CHECK (
        tenant_ref <> ''
        AND tenant_ref COLLATE "pg_unicode_fast" !~ '(^[[:space:]])|([[:space:]]$)'
        AND NOT (
            tenant_ref COLLATE "pg_unicode_fast" ~ '[[:digit:]]'
            AND tenant_ref COLLATE "pg_unicode_fast"
                ~ '^[[:digit:]+,.eE\u066B\u066C\uFF0E\uFF0C-]+$'
        )
    ),
    CONSTRAINT assessment_participant_created_time_positive_check CHECK (
        created_at_unix_ms > 0
    )
);
