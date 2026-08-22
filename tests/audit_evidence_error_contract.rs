//! Stable operator-facing error contracts for immutable audit evidence.

use psychometrics_commons_runtime::audit::AuditEvidenceError;

#[test]
fn construction_errors_expose_stable_messages_without_hidden_sources() {
    for (error, expected_message) in [
        (
            AuditEvidenceError::InvalidReference,
            "audit evidence references must be exact canonical opaque non-numeric values",
        ),
        (
            AuditEvidenceError::InvalidCode,
            "audit purpose and action codes must be lowercase ASCII machine tokens",
        ),
        (
            AuditEvidenceError::InvalidDigest,
            "audit supporting evidence digest must be canonical lowercase SHA-256 evidence",
        ),
        (
            AuditEvidenceError::InvalidTimestamp,
            "audit event timestamp must be positive",
        ),
        (
            AuditEvidenceError::InvalidOutcome,
            "audit outcome code is unsupported",
        ),
    ] {
        assert_eq!(error.to_string(), expected_message);
        assert!(std::error::Error::source(&error).is_none());
    }
}
