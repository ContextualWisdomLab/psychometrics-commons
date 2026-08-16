//! Boundary tests for RFC 9457 problem-type URI and machine-code validation.
//!
//! These cases exercise the public constructor through realistic URI spellings so coverage proves
//! every fail-closed branch without reaching into private implementation helpers.

use psychometrics_commons_runtime::api_problem::{ApiProblem, ApiProblemContractError};

const TITLE: &str = "Public problem";
const DETAIL: &str = "The request could not be completed.";
const CODE: &str = "public_problem_2";

fn problem(type_uri: &'static str) -> Result<ApiProblem, ApiProblemContractError> {
    ApiProblem::new(type_uri, 400, TITLE, DETAIL, CODE)
}

#[test]
fn https_problem_types_cover_authority_port_and_suffix_edges() {
    for valid_type in [
        "https://a",
        "https://a:443",
        "https://example.test/path!$&'()*+,;=:@-._~AZaz09",
        "https://example.test/path?query/with?chars:@!$&'()*+,;=-._~AZaz09#frag/with?chars:@!$&'()*+,;=-._~AZaz09",
        "https://example.test/%2F",
    ] {
        assert!(problem(valid_type).is_ok(), "{valid_type:?} must be accepted");
    }

    for invalid_type in [
        "ftp://example.test/problem",
        "https://",
        "https://user@example.test/problem",
        "https://:80/problem",
        "https://bad!.test/problem",
        "https://example.test:",
        "https://example.test:notaport/problem",
        "https://example.test/%",
        "https://example.test/%G0",
        "https://example.test/%0G",
        "https://example.test/[invalid]",
        "https://example.test?bad value",
        "https://example.test#bad value",
    ] {
        assert_eq!(
            problem(invalid_type),
            Err(ApiProblemContractError::InvalidTypeUri),
            "{invalid_type:?} must fail closed"
        );
    }
}

#[test]
fn urn_problem_types_cover_namespace_and_specific_string_edges() {
    for valid_type in [
        "urn:ab:x",
        "urn:a-b:value",
        "urn:abcdefghijklmnopqrstuvwxyz123456:value",
        "urn:example:a@b/c:d!$&'()*+,;=-._~09AZaz",
        "urn:example:%2F",
    ] {
        assert!(problem(valid_type).is_ok(), "{valid_type:?} must be accepted");
    }

    for invalid_type in [
        "urn:",
        "urn:x:value",
        "urn:abcdefghijklmnopqrstuvwxyz1234567:value",
        "urn:-a:value",
        "urn:a-:value",
        "urn:a_b:value",
        "urn:ab:",
        "urn:ab:bad value",
        "urn:ab:%",
        "urn:ab:%G0",
        "urn:ab:%0G",
    ] {
        assert_eq!(
            problem(invalid_type),
            Err(ApiProblemContractError::InvalidTypeUri),
            "{invalid_type:?} must fail closed"
        );
    }
}

#[test]
fn machine_code_edges_keep_lowercase_ascii_contract_exact() {
    for valid_code in ["a", "a0", "a_b", "public_problem_2"] {
        assert!(ApiProblem::new("urn:ab:x", 400, TITLE, DETAIL, valid_code).is_ok());
    }

    for invalid_code in ["", "0a", "A", "a-b", "a.b", "a b", "å"] {
        assert_eq!(
            ApiProblem::new("urn:ab:x", 400, TITLE, DETAIL, invalid_code),
            Err(ApiProblemContractError::InvalidCode),
            "{invalid_code:?} must fail closed"
        );
    }
}
