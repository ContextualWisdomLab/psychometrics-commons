//! Validation tests for anonymous assessment context construction.

use psychometrics_commons_runtime::anonymous_session::{
    AnonymousSessionContext, AnonymousSessionContextError,
};

#[test]
fn invalid_reference_is_rejected() {
    assert_eq!(
        AnonymousSessionContext::new("", "participant_alpha", "session_alpha", "evidence_alpha", 2_000),
        Err(AnonymousSessionContextError::InvalidReference)
    );
    assert_eq!(
        AnonymousSessionContext::new("tenant_alpha", "", "session_alpha", "evidence_alpha", 2_000),
        Err(AnonymousSessionContextError::InvalidReference)
    );
    assert_eq!(
        AnonymousSessionContext::new("tenant_alpha", "participant_alpha", "", "evidence_alpha", 2_000),
        Err(AnonymousSessionContextError::InvalidReference)
    );
    assert_eq!(
        AnonymousSessionContext::new("tenant_alpha", "participant_alpha", "session_alpha", "", 2_000),
        Err(AnonymousSessionContextError::InvalidReference)
    );
}

#[test]
fn zero_validity_boundary_is_rejected() {
    assert_eq!(
        AnonymousSessionContext::new(
            "tenant_alpha",
            "participant_alpha",
            "session_alpha",
            "evidence_alpha",
            0,
        ),
        Err(AnonymousSessionContextError::InvalidValidityBoundary)
    );
}

#[test]
fn error_messages_are_stable() {
    assert_eq!(
        AnonymousSessionContextError::InvalidReference.to_string(),
        "anonymous-session references must be opaque non-numeric values"
    );
    assert_eq!(
        AnonymousSessionContextError::InvalidValidityBoundary.to_string(),
        "anonymous-session validity boundary must be greater than zero"
    );
}
