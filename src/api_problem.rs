//! Safe problem details for the future public HTTP API without depending on an HTTP framework.
//!
//! RFC 9457 defines a standard problem-details object for HTTP APIs. This module is
//! **transport-neutral**: it defines the error information without deciding how a web server sends
//! bytes over HTTP. Product code first constructs an [`ApiProblem`] from reviewed public values.
//! A future **HTTP adapter**—the small layer that converts domain values into HTTP responses—can
//! then serialize those fields as `application/problem+json`.
//!
//! Title and detail are **occurrence-independent**: their wording describes the problem category,
//! not private data from one failed request. They are `&'static str` so runtime provider, SQL,
//! credential, assessment-response, or other sensitive error text cannot be forwarded by accident.
//! The contract also requires a structurally valid explicit HTTPS or URN problem type instead of
//! relying on `about:blank`.
//!
//! A future adapter may add an opaque **`instance` reference**, meaning an identifier for one
//! particular problem occurrence, and **correlation metadata**, meaning safe identifiers used to
//! connect that occurrence to server-side logs or traces. Those request-specific values stay out
//! of this reusable problem definition.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// RFC 9457 JSON media type used by HTTP adapters that serialize [`ApiProblem`].
pub const PROBLEM_JSON_MEDIA_TYPE: &str = "application/problem+json";

/// Validation error for the public problem-details contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApiProblemContractError {
    /// The problem type was not a structurally valid explicit HTTPS or URN identifier.
    InvalidTypeUri,
    /// The status was outside the HTTP 4xx or 5xx error ranges.
    InvalidStatus,
    /// The human-readable public title was blank.
    EmptyTitle,
    /// The occurrence-independent public detail was blank.
    EmptyDetail,
    /// The stable client machine code was not lowercase ASCII or did not start with a letter.
    InvalidCode,
}

impl Display for ApiProblemContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidTypeUri => {
                "problem type must use a structurally valid explicit HTTPS or URN identifier"
            }
            Self::InvalidStatus => "problem status must be an HTTP client or server error status",
            Self::EmptyTitle => "problem title must contain public-safe text",
            Self::EmptyDetail => "problem detail must contain public-safe text",
            Self::InvalidCode => "problem code must be lowercase ASCII and start with a letter",
        })
    }
}

impl Error for ApiProblemContractError {}

/// One stable, public-safe API problem definition.
///
/// Product code constructs this value from reviewed text that is safe for any client. The title and
/// detail do not describe one specific failure occurrence, so this type cannot hold an arbitrary
/// runtime error. A future HTTP adapter can serialize the value and add an opaque `instance`
/// reference plus safe correlation identifiers for that individual request without exposing
/// implementation details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiProblem {
    type_uri: &'static str,
    status: u16,
    title: &'static str,
    detail: &'static str,
    code: &'static str,
}

impl ApiProblem {
    /// Create one validated public problem definition.
    ///
    /// `type_uri` must be an explicit product identifier using either a structurally valid HTTPS
    /// URI with a non-empty registered-name host or a structurally valid RFC 8141 URN. A URN may
    /// include its optional resolver (`?+`), query (`?=`), and fragment (`#`) components. `status`
    /// must be from 400 through 599. `title` and `detail` must contain non-whitespace static text.
    /// `code` must start with an ASCII lowercase letter and may then contain only ASCII lowercase
    /// letters, digits, or underscores.
    ///
    /// Requiring static title/detail text is intentional: transport code must map internal errors
    /// to reviewed public wording rather than forwarding database, provider, credential, response,
    /// or debugging messages.
    ///
    /// # Errors
    ///
    /// Returns the matching [`ApiProblemContractError`] when any field violates this contract.
    pub fn new(
        type_uri: &'static str,
        status: u16,
        title: &'static str,
        detail: &'static str,
        code: &'static str,
    ) -> Result<Self, ApiProblemContractError> {
        if !valid_problem_type_uri(type_uri) {
            return Err(ApiProblemContractError::InvalidTypeUri);
        }
        if !(400..=599).contains(&status) {
            return Err(ApiProblemContractError::InvalidStatus);
        }
        if title.trim().is_empty() {
            return Err(ApiProblemContractError::EmptyTitle);
        }
        if detail.trim().is_empty() {
            return Err(ApiProblemContractError::EmptyDetail);
        }
        if !valid_machine_code(code) {
            return Err(ApiProblemContractError::InvalidCode);
        }

        Ok(Self {
            type_uri,
            status,
            title,
            detail,
            code,
        })
    }

    /// Return the explicit RFC 9457 problem type identifier.
    #[must_use]
    pub const fn type_uri(&self) -> &'static str {
        self.type_uri
    }

    /// Return the HTTP client/server error status associated with this problem.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Return the short reviewed human-readable title.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        self.title
    }

    /// Return the reviewed public explanation that does not contain runtime error text.
    #[must_use]
    pub const fn detail(&self) -> &'static str {
        self.detail
    }

    /// Return the stable machine-readable extension code for client logic.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Return the RFC 9457 JSON media type used when an HTTP adapter serializes the problem.
    #[must_use]
    pub const fn media_type() -> &'static str {
        PROBLEM_JSON_MEDIA_TYPE
    }
}

fn valid_problem_type_uri(type_uri: &str) -> bool {
    if let Some(remainder) = type_uri.strip_prefix("https://") {
        return valid_https_problem_type(remainder);
    }
    if let Some(remainder) = type_uri.strip_prefix("urn:") {
        return valid_urn_problem_type(remainder);
    }
    false
}

fn valid_https_problem_type(remainder: &str) -> bool {
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }

    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    if !valid_registered_host(host) {
        return false;
    }
    if port.is_some_and(|port| port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }

    valid_https_suffix(&remainder[authority_end..])
}

fn valid_registered_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'~'))
}

fn valid_urn_problem_type(remainder: &str) -> bool {
    let Some((namespace_id, namestring)) = remainder.split_once(':') else {
        return false;
    };
    if !valid_urn_namespace_id(namespace_id) {
        return false;
    }

    // RFC 8141: assigned-name [ "?+" r-component ] [ "?=" q-component ] [ "#" fragment ].
    // The NSS is deliberately parsed before the optional components because a literal '?' is not
    // valid NSS data; it is meaningful only as the beginning of one of those component markers.
    let (assigned_and_rq, fragment) = namestring
        .split_once('#')
        .map_or((namestring, None), |(before, fragment)| {
            (before, Some(fragment))
        });
    if fragment
        .is_some_and(|fragment| !valid_percent_encoded_ascii(fragment, is_query_or_fragment_byte))
    {
        return false;
    }

    let Some(component_start) = assigned_and_rq.find('?') else {
        return valid_urn_namespace_specific_string(assigned_and_rq);
    };
    let namespace_specific_string = &assigned_and_rq[..component_start];
    if !valid_urn_namespace_specific_string(namespace_specific_string) {
        return false;
    }

    let optional_components = &assigned_and_rq[component_start..];
    if let Some(resolver_and_query) = optional_components.strip_prefix("?+") {
        let (resolver, query) = resolver_and_query
            .split_once("?=")
            .map_or((resolver_and_query, None), |(resolver, query)| {
                (resolver, Some(query))
            });
        return valid_urn_rq_component(resolver) && query.is_none_or(valid_urn_rq_component);
    }
    if let Some(query) = optional_components.strip_prefix("?=") {
        return valid_urn_rq_component(query);
    }
    false
}

fn valid_urn_namespace_id(namespace_id: &str) -> bool {
    let bytes = namespace_id.as_bytes();
    if !(2..=32).contains(&bytes.len()) {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..bytes.len() - 1]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

fn valid_urn_namespace_specific_string(value: &str) -> bool {
    valid_urn_component(value, is_path_byte)
}

fn valid_urn_rq_component(value: &str) -> bool {
    valid_urn_component(value, is_query_or_fragment_byte)
}

fn valid_urn_component(value: &str, continuation_allowed: fn(u8) -> bool) -> bool {
    let bytes = value.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };

    let continuation_start = if first == b'%' {
        if bytes.len() < 3 || !bytes[1].is_ascii_hexdigit() || !bytes[2].is_ascii_hexdigit() {
            return false;
        }
        3
    } else if is_path_segment_byte(first) {
        1
    } else {
        return false;
    };

    valid_percent_encoded_ascii(&value[continuation_start..], continuation_allowed)
}

fn valid_https_suffix(value: &str) -> bool {
    let (path_and_query, fragment) = value
        .split_once('#')
        .map_or((value, None), |(before, fragment)| (before, Some(fragment)));
    let (path, query) = path_and_query
        .split_once('?')
        .map_or((path_and_query, None), |(path, query)| (path, Some(query)));

    valid_percent_encoded_ascii(path, is_path_byte)
        && query.is_none_or(|query| valid_percent_encoded_ascii(query, is_query_or_fragment_byte))
        && fragment
            .is_none_or(|fragment| valid_percent_encoded_ascii(fragment, is_query_or_fragment_byte))
}

const fn is_path_byte(byte: u8) -> bool {
    is_path_segment_byte(byte) || byte == b'/'
}

const fn is_query_or_fragment_byte(byte: u8) -> bool {
    is_path_segment_byte(byte) || matches!(byte, b'/' | b'?')
}

const fn is_path_segment_byte(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delimiter(byte) || matches!(byte, b':' | b'@')
}

fn valid_percent_encoded_ascii(value: &str, allowed: fn(u8) -> bool) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        if !allowed(byte) {
            return false;
        }
        index += 1;
    }
    true
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const fn is_sub_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn valid_machine_code(code: &str) -> bool {
    let Some(first) = code.as_bytes().first() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    code.bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{ApiProblem, ApiProblemContractError};

    #[test]
    fn contract_errors_and_uri_branches_are_instantiated_in_the_library() {
        for error in [
            ApiProblemContractError::InvalidTypeUri,
            ApiProblemContractError::InvalidStatus,
            ApiProblemContractError::EmptyTitle,
            ApiProblemContractError::EmptyDetail,
            ApiProblemContractError::InvalidCode,
        ] {
            assert!(!error.to_string().is_empty());
        }

        let problem = ApiProblem::new(
            "https://example.test/problems/denied",
            403,
            "Denied",
            "The request is outside the authorized tenant.",
            "cross_tenant_denied",
        )
        .unwrap();
        assert_eq!(problem.type_uri(), "https://example.test/problems/denied");
        assert_eq!(problem.status(), 403);
        assert_eq!(problem.title(), "Denied");
        assert_eq!(
            problem.detail(),
            "The request is outside the authorized tenant."
        );
        assert_eq!(problem.code(), "cross_tenant_denied");
        assert_eq!(ApiProblem::media_type(), "application/problem+json");

        for invalid_type in [
            "https://bad!.test/problems/denied",
            "https://:80/problems/denied",
            "https://example.test:",
            "https://example.test:abc/problems/denied",
            "https://user@example.test/problems/denied",
            "https://example.test/%",
            "https://example.test/%0G",
            "https://example.test/?%zz",
            "https://example.test#%",
            "urn:-a:value",
            "urn:a-:value",
            "urn:example:bad value",
            "urn:example:/problem",
            "urn:example:%",
            "urn:example:%0",
            "urn:example:%0G",
            "urn:example:problem?bare",
            "urn:example:problem?+",
            "urn:example:problem?=",
            "urn:example:problem?+/resolver",
            "urn:example:problem?+?resolver",
            "urn:example:problem?=/version",
            "urn:example:problem?=?version",
            "urn:example:problem#one#two",
            "urn:example:problem?+resolver?=%zz",
        ] {
            assert_eq!(
                ApiProblem::new(invalid_type, 403, "Denied", "Public detail.", "denied"),
                Err(ApiProblemContractError::InvalidTypeUri),
                "{invalid_type}"
            );
        }
        for valid_type in [
            "https://example.test:443/problems/denied",
            "https://example.test/problems/denied?version=2#details",
            "https://example.test?a/b?c",
            "urn:ab:value",
            "urn:example:%70roblem",
            "urn:example:problem:v1",
            "urn:example:problem/v1",
            "urn:example:a@b",
            "urn:example:problem#details",
            "urn:example:problem?=version=2",
            "urn:example:problem?+resolver=primary",
            "urn:example:problem?+resolver=primary?=version=2#details",
        ] {
            assert!(
                ApiProblem::new(valid_type, 403, "Denied", "Public detail.", "denied").is_ok(),
                "{valid_type}"
            );
        }
    }
}
