//! Machine-readable contract gate for the persist-backed session HTTP boundary.
//!
//! ADR-0014 requires every implemented HTTP operation to carry an exact
//! OpenAPI 3.2.x as-built contract. This test includes the session contract at
//! compile time and checks the implemented collection path, methods, response
//! families, and durable session representation so contract drift fails CI.

use psychometrics_commons_runtime::session_http::SESSION_COLLECTION_PATH;

const SESSION_OPENAPI: &str = include_str!("../openapi/sessions.yaml");

fn section<'a>(document: &'a str, start: &str, end: Option<&str>) -> &'a str {
    let (_, remainder) = document
        .split_once(start)
        .unwrap_or_else(|| panic!("missing OpenAPI section: {start:?}"));
    match end {
        Some(end) => remainder
            .split_once(end)
            .map_or(remainder, |(body, _)| body),
        None => remainder,
    }
}

#[test]
fn session_openapi_is_pinned_to_the_implemented_operations() {
    assert_eq!(SESSION_COLLECTION_PATH, "/v1/sessions");
    assert!(SESSION_OPENAPI.starts_with("openapi: 3.2.0\n"));
    assert_eq!(
        SESSION_OPENAPI.matches("\n  /v1/").count(),
        2,
        "session OpenAPI must describe only the two implemented session paths"
    );

    let create = section(
        SESSION_OPENAPI,
        "  /v1/sessions:\n",
        Some("  /v1/sessions/{session_ref}:\n"),
    );
    for required_fragment in [
        "    post:\n",
        "      operationId: startAssessmentSession\n",
        "        - name: Idempotency-Key\n",
        "          required: true\n",
        "        \"200\":\n",
        "        \"201\":\n",
        "        \"400\":\n",
        "        \"405\":\n",
        "        \"409\":\n",
        "        \"500\":\n",
        "#/components/schemas/SessionCreate",
        "#/components/schemas/CreatedSession",
    ] {
        assert!(
            create.contains(required_fragment),
            "missing as-built session-create contract fragment: {required_fragment:?}"
        );
    }

    let reload = section(
        SESSION_OPENAPI,
        "  /v1/sessions/{session_ref}:\n",
        Some("components:\n"),
    );
    for required_fragment in [
        "    get:\n",
        "      operationId: loadAssessmentSession\n",
        "        - name: session_ref\n",
        "          in: path\n",
        "          required: true\n",
        "        \"200\":\n",
        "        \"400\":\n",
        "        \"404\":\n",
        "        \"405\":\n",
        "        \"500\":\n",
        "#/components/schemas/CreatedSession",
    ] {
        assert!(
            reload.contains(required_fragment),
            "missing as-built session-reload contract fragment: {required_fragment:?}"
        );
    }
}

#[test]
fn session_openapi_preserves_durable_session_identity_and_problem_shape() {
    let components = section(SESSION_OPENAPI, "components:\n", None);
    for required_property in [
        "        participant_ref:\n",
        "        instrument_release_ref:\n",
        "        locale:\n",
        "        session_ref:\n",
        "        instrument_version_ref:\n",
        "        instrument_release_content_digest:\n",
        "        state:\n",
        "        created_at_unix_ms:\n",
    ] {
        assert!(
            components.contains(required_property),
            "missing session identity/provenance property from OpenAPI: {required_property:?}"
        );
    }

    for required_fragment in [
        "        application/problem+json:\n",
        "        - type\n",
        "        - title\n",
        "        - status\n",
        "        - detail\n",
    ] {
        assert!(
            components.contains(required_fragment),
            "missing RFC 9457 problem contract fragment: {required_fragment:?}"
        );
    }
}
