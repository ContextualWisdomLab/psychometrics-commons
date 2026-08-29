//! Machine-readable contract gate for the personal result-export HTTP boundary.
//!
//! ADR-0014 requires every implemented HTTP operation to carry an exact
//! `OpenAPI` 3.2.x as-built contract in the same or a prerequisite change. This
//! test includes the contract at compile time so deleting or deferring the
//! artifact makes the result-export slice fail closed.

const RESULT_EXPORT_OPENAPI: &str = include_str!("../openapi/result-exports.yaml");

#[test]
fn result_export_openapi_is_pinned_to_the_implemented_operation() {
    assert!(RESULT_EXPORT_OPENAPI.starts_with("openapi: 3.2.0\n"));
    for required_fragment in [
        "  /v1/results/{result_ref}/exports:\n",
        "    post:\n",
        "      operationId: deliverPersonalResultExport\n",
        "      x-authorization-contract: >\n",
        "        - name: Idempotency-Key\n",
        "        \"200\":\n",
        "        \"400\":\n",
        "        \"403\":\n",
        "        \"404\":\n",
        "        \"405\":\n",
        "        \"406\":\n",
        "        \"409\":\n",
        "        application/problem+json:\n",
    ] {
        assert!(
            RESULT_EXPORT_OPENAPI.contains(required_fragment),
            "missing as-built result-export contract fragment: {required_fragment:?}"
        );
    }
}

#[test]
fn result_export_openapi_constrains_request_identity_and_score_states() {
    let opaque_reference_binding = "$ref: \"#/components/schemas/OpaqueReference\"";
    assert!(
        RESULT_EXPORT_OPENAPI
            .matches(opaque_reference_binding)
            .count()
            >= 2,
        "result_ref and Idempotency-Key must both reuse the opaque-reference schema"
    );

    for required_fragment in [
        "    OpaqueReference:\n",
        "      x-runtime-validator: exact_opaque_reference\n",
        "      minLength: 1\n",
        "      allOf:\n",
        "    ScoreObservation:\n",
        "      oneOf:\n",
        "        - title: Scored observation\n",
        "              const: scored\n",
        "        - title: Non-scored observation\n",
        "                - abstained\n",
        "                - failed\n",
        "                - excluded\n",
        "              type: \"null\"\n",
    ] {
        assert!(
            RESULT_EXPORT_OPENAPI.contains(required_fragment),
            "missing fail-closed result-export schema constraint: {required_fragment:?}"
        );
    }
}

#[test]
fn result_export_openapi_preserves_immutable_scoring_provenance() {
    for required_property in [
        "        export_ref:\n",
        "        result_snapshot_ref:\n",
        "        participant_ref:\n",
        "        session_ref:\n",
        "        response_snapshot_ref:\n",
        "        assessment_spec_ref:\n",
        "        instrument_version_ref:\n",
        "        scoring_version_ref:\n",
        "        calibration_reference:\n",
        "        norm_version_ref:\n",
        "        narrative_version_ref:\n",
        "        engine_artifact_digest:\n",
        "        requested_output_schema_version:\n",
        "        locale:\n",
        "        created_at_unix_ms:\n",
        "        exported_at_unix_ms:\n",
        "        consent_snapshot_refs:\n",
        "        score_observations:\n",
        "        limitations:\n",
        "        construct_ref:\n",
        "        disposition:\n",
        "        score:\n",
        "        standard_error:\n",
    ] {
        assert!(
            RESULT_EXPORT_OPENAPI.contains(required_property),
            "missing result-export provenance property from OpenAPI: {required_property:?}"
        );
    }

    for disposition in ["scored", "abstained", "failed", "excluded"] {
        assert!(
            RESULT_EXPORT_OPENAPI.contains(&format!("            - {disposition}\n")),
            "missing wire disposition {disposition}"
        );
    }
}

#[test]
fn result_export_openapi_names_the_stable_problem_types() {
    for problem_type in [
        "urn:psychometrics-commons:problem:bad-request",
        "urn:psychometrics-commons:problem:unsupported-query",
        "urn:psychometrics-commons:problem:result-export-forbidden",
        "urn:psychometrics-commons:problem:not-found",
        "urn:psychometrics-commons:problem:method-not-allowed",
        "urn:psychometrics-commons:problem:not-acceptable",
        "urn:psychometrics-commons:problem:idempotency-conflict",
    ] {
        assert!(
            RESULT_EXPORT_OPENAPI.contains(problem_type),
            "missing implemented RFC 9457 problem type {problem_type}"
        );
    }
}
