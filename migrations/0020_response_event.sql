DO $response_event_schema$
DECLARE
    relation_ref REGCLASS := to_regclass('response_event');
    created_table BOOLEAN := relation_ref IS NULL;
    expected_relation_ref REGCLASS;
    actual_columns TEXT[];
    expected_columns TEXT[];
    actual_defaults TEXT[];
    expected_defaults TEXT[];
    actual_constraints TEXT[];
    expected_constraints TEXT[];
BEGIN
    IF created_table THEN
        EXECUTE $create_response_event$
CREATE TABLE response_event (
    response_event_ref TEXT CONSTRAINT response_event_response_event_ref_not_null NOT NULL
        CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_ref = btrim(response_event_ref)
            AND response_event_ref <> ''
            AND NOT (
                response_event_ref ~ '[[:digit:]]'
                AND response_event_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL
        CONSTRAINT response_event_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    client_event_ref TEXT CONSTRAINT response_event_client_event_ref_not_null NOT NULL
        CONSTRAINT response_event_client_event_ref_format_check CHECK (
            client_event_ref = btrim(client_event_ref)
            AND client_event_ref <> ''
            AND NOT (
                client_event_ref ~ '[[:digit:]]'
                AND client_event_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    item_version_ref TEXT CONSTRAINT response_event_item_version_ref_not_null NOT NULL
        CONSTRAINT response_event_item_version_ref_format_check CHECK (
            item_version_ref = btrim(item_version_ref)
            AND item_version_ref <> ''
            AND NOT (
                item_version_ref ~ '[[:digit:]]'
                AND item_version_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    payload_digest TEXT CONSTRAINT response_event_payload_digest_not_null NOT NULL
        CONSTRAINT response_event_payload_digest_format_check CHECK (
            payload_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    observed_at TIMESTAMPTZ CONSTRAINT response_event_observed_at_not_null NOT NULL,
    received_at TIMESTAMPTZ CONSTRAINT response_event_received_at_not_null NOT NULL,
    CONSTRAINT response_event_pkey PRIMARY KEY (response_event_ref),
    CONSTRAINT response_event_session_client_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_session_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_observed_not_after_received_check CHECK (observed_at <= received_at)
)
$create_response_event$;
        relation_ref := to_regclass('response_event');
    END IF;

    IF relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event migration did not create its owned table';
    END IF;

    EXECUTE $create_expected_response_event$
CREATE TEMP TABLE expected_response_event_contract (
    response_event_ref TEXT CONSTRAINT response_event_response_event_ref_not_null NOT NULL
        CONSTRAINT response_event_response_event_ref_format_check CHECK (
            response_event_ref = btrim(response_event_ref)
            AND response_event_ref <> ''
            AND NOT (
                response_event_ref ~ '[[:digit:]]'
                AND response_event_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    session_ref TEXT CONSTRAINT response_event_session_ref_not_null NOT NULL
        CONSTRAINT response_event_session_ref_format_check CHECK (
            session_ref = btrim(session_ref)
            AND session_ref <> ''
            AND NOT (
                session_ref ~ '[[:digit:]]'
                AND session_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    client_event_ref TEXT CONSTRAINT response_event_client_event_ref_not_null NOT NULL
        CONSTRAINT response_event_client_event_ref_format_check CHECK (
            client_event_ref = btrim(client_event_ref)
            AND client_event_ref <> ''
            AND NOT (
                client_event_ref ~ '[[:digit:]]'
                AND client_event_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    item_version_ref TEXT CONSTRAINT response_event_item_version_ref_not_null NOT NULL
        CONSTRAINT response_event_item_version_ref_format_check CHECK (
            item_version_ref = btrim(item_version_ref)
            AND item_version_ref <> ''
            AND NOT (
                item_version_ref ~ '[[:digit:]]'
                AND item_version_ref ~ '^[[:digit:]+,.eE．٫٬，-]+$'
            )
        ),
    payload_digest TEXT CONSTRAINT response_event_payload_digest_not_null NOT NULL
        CONSTRAINT response_event_payload_digest_format_check CHECK (
            payload_digest ~ '^sha256:[0-9a-f]{64}$'
        ),
    server_sequence BIGINT CONSTRAINT response_event_server_sequence_not_null NOT NULL
        CONSTRAINT response_event_server_sequence_positive_check CHECK (server_sequence > 0),
    observed_at TIMESTAMPTZ CONSTRAINT response_event_observed_at_not_null NOT NULL,
    received_at TIMESTAMPTZ CONSTRAINT response_event_received_at_not_null NOT NULL,
    CONSTRAINT response_event_pkey PRIMARY KEY (response_event_ref),
    CONSTRAINT response_event_session_client_unique UNIQUE (session_ref, client_event_ref),
    CONSTRAINT response_event_session_sequence_unique UNIQUE (session_ref, server_sequence),
    CONSTRAINT response_event_observed_not_after_received_check CHECK (observed_at <= received_at)
) ON COMMIT DROP
$create_expected_response_event$;
    expected_relation_ref := to_regclass('pg_temp.expected_response_event_contract');

    IF expected_relation_ref IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event migration could not construct its schema contract';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s',
            attribute.attname,
            format_type(attribute.atttypid, attribute.atttypmod),
            CASE WHEN attribute.attnotnull THEN 'not_null' ELSE 'nullable' END
        )
        FROM pg_attribute AS attribute
        WHERE attribute.attrelid = relation_ref
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
        ORDER BY attribute.attnum
    ) INTO actual_columns;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s',
            attribute.attname,
            format_type(attribute.atttypid, attribute.atttypmod),
            CASE WHEN attribute.attnotnull THEN 'not_null' ELSE 'nullable' END
        )
        FROM pg_attribute AS attribute
        WHERE attribute.attrelid = expected_relation_ref
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
        ORDER BY attribute.attnum
    ) INTO expected_columns;

    IF actual_columns IS DISTINCT FROM expected_columns THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event column contract does not match migration 0020';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            attribute.attname,
            pg_get_expr(default_value.adbin, default_value.adrelid)
        )
        FROM pg_attribute AS attribute
        JOIN pg_attrdef AS default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        WHERE attribute.attrelid = relation_ref
        ORDER BY attribute.attnum
    ) INTO actual_defaults;

    SELECT ARRAY(
        SELECT format(
            '%s:%s',
            attribute.attname,
            pg_get_expr(default_value.adbin, default_value.adrelid)
        )
        FROM pg_attribute AS attribute
        JOIN pg_attrdef AS default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        WHERE attribute.attrelid = expected_relation_ref
        ORDER BY attribute.attnum
    ) INTO expected_defaults;

    IF actual_defaults IS DISTINCT FROM expected_defaults THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event default contract does not match migration 0020';
    END IF;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s:%s:%s',
            constraint_record.conname,
            constraint_record.contype,
            constraint_record.convalidated,
            constraint_record.conenforced,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = relation_ref
          AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO actual_constraints;

    SELECT ARRAY(
        SELECT format(
            '%s:%s:%s:%s:%s',
            constraint_record.conname,
            constraint_record.contype,
            constraint_record.convalidated,
            constraint_record.conenforced,
            pg_get_constraintdef(constraint_record.oid)
        )
        FROM pg_constraint AS constraint_record
        WHERE constraint_record.conrelid = expected_relation_ref
          AND constraint_record.contype IN ('c', 'f', 'n', 'p', 'u', 'x')
        ORDER BY constraint_record.conname
    ) INTO expected_constraints;

    IF actual_constraints IS DISTINCT FROM expected_constraints THEN
        RAISE EXCEPTION USING
            ERRCODE = '55000',
            MESSAGE = 'response_event constraint contract does not match migration 0020';
    END IF;
END
$response_event_schema$;
