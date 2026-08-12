//! Regression coverage for canonical integration payload digests.

use psychometrics_commons_runtime::integration::{IntegrationError, IntegrationEvent};

const DIGEST_WITHOUT_PREFIX: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn digest_without_algorithm_prefix_fails_closed() {
    let result = IntegrationEvent::new(
        "event_alpha",
        "assessment.scoring.requested",
        "v1",
        "psychometrics_commons",
        "tenant_alpha",
        "session_alpha",
        10_000,
        "correlation_alpha",
        None,
        DIGEST_WITHOUT_PREFIX,
    );

    assert_eq!(result, Err(IntegrationError::InvalidDigest));
}
