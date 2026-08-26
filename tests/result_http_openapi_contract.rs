//! Machine-readable contract gate for the immutable result-read HTTP boundary.
//!
//! ADR-0014 requires every implemented HTTP operation to carry an exact
//! `OpenAPI` 3.2.x as-built contract in the same or a prerequisite change. This
//! test intentionally includes the contract at compile time so deleting or
//! deferring the artifact makes the result-read slice fail closed.

const RESULT_OPENAPI: &str = include_str!("../openapi/results.yaml");

#[test]
fn result_read_openapi_is_pinned_to_the_implemented_operation() {
    assert!(RESULT_OPENAPI.starts_with("openapi: 3.2.0\n"));
    for required_fragment in [
        "  /v1/results/{result_ref}:\n",
        "    get:\n",
        "      operationId: getImmutableResult\n",
        "        \"200\":\n",
        "        \"400\":\n",
        "        \"403\":\n",
        "        \"404\":\n",
        "        \"405\":\n",
        "        application/problem+json:\n",
        "      x-authorization-contract: >\n",
    ] {
        assert!(
            RESULT_OPENAPI.contains(required_fragment),
            "missing as-built result HTTP contract fragment: {required_fragment:?}"
        );
    }
}

#[test]
fn result_read_openapi_declares_the_method_not_allowed_allow_header() {
    for required_fragment in [
        "        \"405\":\n",
        "          headers:\n",
        "            Allow:\n",
        "                const: GET\n",
    ] {
        assert!(
            RESULT_OPENAPI.contains(required_fragment),
            "missing RFC 9110 method response contract fragment: {required_fragment:?}"
        );
    }
}

#[test]
fn result_read_openapi_preserves_immutable_scoring_provenance() {
    for required_property in [
        "        result_ref:\n",
        "        participant_ref:\n",
        "        session_ref:\n",
        "        response_snapshot_ref:\n",
        "        assessment_spec_ref:\n",
        "        instrument_version_ref:\n",
        "        scoring_version_ref:\n",
        "        calibration_reference:\n",
        "        norm_version_ref:\n",
        "        narrative_version_ref:\n",
        "        consent_snapshot_refs:\n",
        "        engine_artifact_digest:\n",
        "        requested_output_schema_version:\n",
        "        created_at_unix_ms:\n",
        "        supersedes_ref:\n",
        "        score_observations:\n",
        "        construct_ref:\n",
        "        disposition:\n",
        "        score:\n",
        "        standard_error:\n",
    ] {
        assert!(
            RESULT_OPENAPI.contains(required_property),
            "missing result provenance property from OpenAPI: {required_property:?}"
        );
    }

    for disposition in ["scored", "abstained", "failed", "excluded"] {
        assert!(
            RESULT_OPENAPI.contains(&format!("          - {disposition}\n")),
            "missing wire disposition {disposition}"
        );
    }
}

#[test]
fn result_read_openapi_names_the_stable_problem_types() {
    for problem_type in [
        "urn:psychometrics-commons:problem:bad-request",
        "urn:psychometrics-commons:problem:unsupported-query",
        "urn:psychometrics-commons:problem:not-found",
        "urn:psychometrics-commons:problem:method-not-allowed",
        "urn:psychometrics-commons:problem:invalid-reference",
        "urn:psychometrics-commons:problem:result-access-denied",
        "urn:psychometrics-commons:problem:result-not-found",
    ] {
        assert!(
            RESULT_OPENAPI.contains(problem_type),
            "missing implemented RFC 9457 problem type {problem_type}"
        );
    }
}
