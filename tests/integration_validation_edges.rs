//! Boundary regressions for integration envelope validation.

use psychometrics_commons_runtime::integration::{IntegrationError, IntegrationEvent};

const VALID_DIGEST: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn create(
    event_type: &str,
    schema_version: &str,
    digest: &str,
) -> Result<IntegrationEvent, IntegrationError> {
    IntegrationEvent::new(
        "event_alpha",
        event_type,
        schema_version,
        "psychometrics_commons",
        "tenant_alpha",
        "subject_alpha",
        10_000,
        "correlation_alpha",
        None,
        digest,
    )
}

#[test]
fn event_type_and_schema_version_are_bounded_and_exact() {
    let long_event_type = "e".repeat(129);
    let long_schema_version = "v".repeat(65);

    assert_eq!(
        create(&long_event_type, "v1", VALID_DIGEST),
        Err(IntegrationError::InvalidEventType)
    );
    assert_eq!(
        create("assessment.completed", &long_schema_version, VALID_DIGEST),
        Err(IntegrationError::InvalidSchemaVersion)
    );

    for event_type in [
        " assessment.completed",
        "assessment.completed ",
        "assessment.\tcompleted",
        "assessment.\ncompleted",
        "assessment.\u{00a0}completed",
    ] {
        assert_eq!(
            create(event_type, "v1", VALID_DIGEST),
            Err(IntegrationError::InvalidEventType),
            "event type {event_type:?} must not normalize onto another contract identity"
        );
    }

    for schema_version in [" v1", "v1 ", "v\t1", "v\n1", "v\u{00a0}1"] {
        assert_eq!(
            create("assessment.completed", schema_version, VALID_DIGEST),
            Err(IntegrationError::InvalidSchemaVersion),
            "schema version {schema_version:?} must not normalize onto another contract identity"
        );
    }

    let max_event_type = "e".repeat(128);
    let max_schema_version = "v".repeat(64);
    let at_limit = create(&max_event_type, &max_schema_version, VALID_DIGEST).unwrap();
    assert_eq!(at_limit.event_type(), max_event_type);
    assert_eq!(at_limit.schema_version(), max_schema_version);
}

#[test]
fn digest_requires_prefix_exact_length_and_lowercase_hex() {
    let uppercase = format!("sha256:{}A", "0".repeat(63));
    let too_long = format!("sha256:{}", "0".repeat(65));

    for digest in [
        "md5:0123456789abcdef0123456789abcdef",
        "sha256:abcd",
        uppercase.as_str(),
        too_long.as_str(),
    ] {
        assert_eq!(
            create("assessment.completed", "v1", digest),
            Err(IntegrationError::InvalidDigest),
            "digest {digest:?} must fail closed"
        );
    }
}
