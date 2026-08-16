//! Transport-neutral `POST /v1/sessions` mapping for starting a created session.
//!
//! A buyer starts an anonymous assessment by sending one published instrument
//! release, an exact locale, and opaque session/participant references. This
//! module turns that request into a created [`AssessmentSession`] or a reviewed
//! `RFC 9457` problem. A future HTTP adapter serializes the accepted body as
//! `application/json` and problems as `application/problem+json`.
//!
//! `session_ref` is the resource-specific idempotency key required by TRD §18
//! and ADR-0014. Exact replay of the same created identity is safe; a later
//! persist adapter treats conflicting rebinding as fail-closed replay.

use crate::api_problem::ApiProblem;
use crate::instrument::InstrumentRelease;
use crate::session::{AssessmentSession, SessionCreationError, SessionState};

/// HTTP method for the as-built session-creation operation.
pub const CREATE_SESSION_HTTP_METHOD: &str = "POST";

/// HTTP path for the as-built session-creation operation.
pub const CREATE_SESSION_HTTP_PATH: &str = "/v1/sessions";

/// Success status an HTTP adapter must use for a newly created session.
pub const CREATE_SESSION_SUCCESS_STATUS: u16 = 201;

/// JSON media type an HTTP adapter uses for a successful create-session body.
pub const CREATE_SESSION_JSON_MEDIA_TYPE: &str = "application/json";

/// Public request values for `POST /v1/sessions`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateSessionHttpRequest<'a> {
    session_ref: &'a str,
    participant_ref: &'a str,
    requested_locale: &'a str,
    created_at_unix_ms: u64,
}

impl<'a> CreateSessionHttpRequest<'a> {
    /// Collect the public create-session fields before domain validation.
    #[must_use]
    pub const fn new(
        session_ref: &'a str,
        participant_ref: &'a str,
        requested_locale: &'a str,
        created_at_unix_ms: u64,
    ) -> Self {
        Self {
            session_ref,
            participant_ref,
            requested_locale,
            created_at_unix_ms,
        }
    }

    /// Return the caller-supplied session reference.
    #[must_use]
    pub const fn session_ref(self) -> &'a str {
        self.session_ref
    }

    /// Return the caller-supplied participant reference.
    #[must_use]
    pub const fn participant_ref(self) -> &'a str {
        self.participant_ref
    }

    /// Return the exact locale the caller asked to assess in.
    #[must_use]
    pub const fn requested_locale(self) -> &'a str {
        self.requested_locale
    }

    /// Return the server-issued creation timestamp in Unix milliseconds.
    #[must_use]
    pub const fn created_at_unix_ms(self) -> u64 {
        self.created_at_unix_ms
    }
}

/// Public accepted body for a created assessment session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionHttpAccepted {
    session: AssessmentSession,
}

impl CreateSessionHttpAccepted {
    /// Return the opaque session reference the client should store and reuse.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        self.session.session_ref()
    }

    /// Return the opaque participant reference bound to this session.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        self.session.participant_ref()
    }

    /// Return the published instrument-release reference copied at creation.
    #[must_use]
    pub fn instrument_release_ref(&self) -> &str {
        self.session.instrument_release_ref()
    }

    /// Return the instrument-version reference copied at creation.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        self.session.instrument_version_ref()
    }

    /// Return the immutable release content digest copied at creation.
    #[must_use]
    pub fn instrument_release_content_digest(&self) -> &str {
        self.session.instrument_release_content_digest()
    }

    /// Return the exact locale copied from the published release.
    #[must_use]
    pub fn locale(&self) -> &str {
        self.session.locale()
    }

    /// Return the server-issued creation timestamp in Unix milliseconds.
    #[must_use]
    pub fn created_at_unix_ms(&self) -> u64 {
        self.session.created_at_unix_ms()
    }

    /// Return the created lifecycle state. Only Created is accepted here.
    #[must_use]
    pub fn session_state(&self) -> SessionState {
        self.session.state()
    }

    /// Return the created session for a later persist adapter.
    #[must_use]
    pub fn session(&self) -> &AssessmentSession {
        &self.session
    }
}

/// Start one created assessment session from a published release.
///
/// Use this from an HTTP adapter that implements `POST /v1/sessions`. On
/// success, persist [`CreateSessionHttpAccepted::session`] with the existing
/// assessment-session adapter, then return HTTP 201 and the accepted fields.
/// On failure, return the problem status and serialize the problem as
/// `application/problem+json` without forwarding SQL or provider text.
///
/// # Errors
///
/// Returns a reviewed [`ApiProblem`] when the session or participant reference
/// is not opaque, the timestamp is zero, the release cannot accept new
/// sessions, or the requested locale is not the published locale.
pub fn create_assessment_session(
    request: CreateSessionHttpRequest<'_>,
    release: &InstrumentRelease,
) -> Result<CreateSessionHttpAccepted, ApiProblem> {
    AssessmentSession::new(
        request.session_ref(),
        request.participant_ref(),
        release,
        request.requested_locale(),
        request.created_at_unix_ms(),
    )
    .map(|session| CreateSessionHttpAccepted { session })
    .map_err(problem_for_creation)
}

fn problem_for_creation(error: SessionCreationError) -> ApiProblem {
    match error {
        SessionCreationError::InvalidReference => catalog_problem(
            "urn:psychometrics-commons:problem:invalid-session-reference",
            400,
            "Invalid session reference",
            "Use an opaque non-numeric session and participant reference.",
            "invalid_session_reference",
        ),
        SessionCreationError::InvalidTimestamp => catalog_problem(
            "urn:psychometrics-commons:problem:invalid-session-timestamp",
            400,
            "Invalid session timestamp",
            "Send a server-issued creation time greater than zero.",
            "invalid_session_timestamp",
        ),
        SessionCreationError::InstrumentReleaseUnavailable => catalog_problem(
            "urn:psychometrics-commons:problem:instrument-release-unavailable",
            409,
            "Instrument release unavailable",
            "Publish this instrument release before starting a new session.",
            "instrument_release_unavailable",
        ),
        SessionCreationError::LocaleMismatch => catalog_problem(
            "urn:psychometrics-commons:problem:locale-mismatch",
            409,
            "Locale mismatch",
            "Request the exact locale published on this instrument release.",
            "locale_mismatch",
        ),
    }
}

fn catalog_problem(
    type_uri: &'static str,
    status: u16,
    title: &'static str,
    detail: &'static str,
    code: &'static str,
) -> ApiProblem {
    ApiProblem::new(type_uri, status, title, detail, code).unwrap_or_else(|error| {
        unreachable!("reviewed session HTTP problem catalog is invalid: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        catalog_problem, problem_for_creation, CreateSessionHttpRequest,
        CREATE_SESSION_HTTP_METHOD, CREATE_SESSION_HTTP_PATH, CREATE_SESSION_JSON_MEDIA_TYPE,
        CREATE_SESSION_SUCCESS_STATUS,
    };
    use crate::session::SessionCreationError;

    #[test]
    fn request_accessors_and_http_constants_are_stable() {
        let request = CreateSessionHttpRequest::new("ses_a", "ptc_b", "ko-KR", 9);
        assert_eq!(request.session_ref(), "ses_a");
        assert_eq!(request.participant_ref(), "ptc_b");
        assert_eq!(request.requested_locale(), "ko-KR");
        assert_eq!(request.created_at_unix_ms(), 9);
        assert_eq!(CREATE_SESSION_HTTP_METHOD, "POST");
        assert_eq!(CREATE_SESSION_HTTP_PATH, "/v1/sessions");
        assert_eq!(CREATE_SESSION_SUCCESS_STATUS, 201);
        assert_eq!(CREATE_SESSION_JSON_MEDIA_TYPE, "application/json");
    }

    #[test]
    fn every_creation_error_maps_to_a_reviewed_problem() {
        for error in [
            SessionCreationError::InvalidReference,
            SessionCreationError::InvalidTimestamp,
            SessionCreationError::InstrumentReleaseUnavailable,
            SessionCreationError::LocaleMismatch,
        ] {
            let problem = problem_for_creation(error);
            assert!((400..=599).contains(&problem.status()));
            assert!(!problem.code().is_empty());
            assert!(!problem.detail().is_empty());
        }
    }

    #[test]
    fn catalog_construction_covers_the_invalid_guard() {
        let problem = catalog_problem(
            "urn:psychometrics-commons:problem:invalid-session-reference",
            400,
            "Invalid session reference",
            "Use an opaque non-numeric session and participant reference.",
            "invalid_session_reference",
        );
        assert_eq!(problem.code(), "invalid_session_reference");
        let panicked = std::panic::catch_unwind(|| {
            catalog_problem("about:blank", 400, "Title", "Detail.", "code");
        });
        assert!(panicked.is_err());
    }
}
