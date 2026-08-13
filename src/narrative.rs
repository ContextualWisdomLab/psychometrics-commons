//! Deterministic presentation identity primitives for Personality Style narrative.
//!
//! Personality Style is a presentation mapping over pinned scientific score evidence. This
//! module does not calculate psychometric scores and does not let model/provider identity
//! alter deterministic style assignment. It only defines the canonical behavior-affecting
//! identity bytes that ADR-0018 requires before persistence or public APIs are introduced.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

const STYLE_ASSIGNMENT_IDENTITY_DOMAIN: &[u8] =
    b"psychometrics-commons/style-assignment-identity/v1\0";

const GRANDFATHERED_LANGUAGE_TAGS: &[&str] = &[
    "art-lojban",
    "cel-gaulish",
    "en-GB-oed",
    "i-ami",
    "i-bnn",
    "i-default",
    "i-enochian",
    "i-hak",
    "i-klingon",
    "i-lux",
    "i-mingo",
    "i-navajo",
    "i-pwn",
    "i-tao",
    "i-tay",
    "i-tsu",
    "no-bok",
    "no-nyn",
    "sgn-BE-FR",
    "sgn-BE-NL",
    "sgn-CH-DE",
    "zh-guoyu",
    "zh-hakka",
    "zh-min",
    "zh-min-nan",
    "zh-xiang",
];

/// Scientific score evidence that a deterministic Personality Style mapping consumes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScoreIdentity<'a> {
    /// Opaque reference to one immutable score profile.
    ScoreProfileRef(&'a str),
    /// Exact digest of an inline canonical score payload when no profile reference exists.
    CanonicalScorePayloadDigest(&'a str),
}

/// All behavior-affecting inputs that determine one deterministic style assignment.
///
/// Model, provider, prompt, and generated wording identity are deliberately absent. They may
/// affect optional narrative wording but cannot change which deterministic style assignment
/// is selected from pinned scientific evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StyleAssignmentIdentity<'a> {
    /// Immutable score evidence consumed by the mapping.
    pub score_identity: ScoreIdentity<'a>,
    /// Published instrument version that produced the score evidence.
    pub instrument_version_ref: &'a str,
    /// Exact scoring contract version used for the score evidence.
    pub scoring_version_ref: &'a str,
    /// Optional norm version used by the presentation mapping.
    pub norm_version_ref: Option<&'a str>,
    /// Deterministic style-mapping version.
    pub style_mapping_version_ref: &'a str,
    /// Exact digest of the approved interpretation-rule bundle.
    pub interpretation_rule_bundle_digest: &'a str,
    /// Exact BCP 47 locale token used by the deterministic presentation contract.
    pub locale: &'a str,
}

/// Fail-closed validation error for canonical style-assignment identity construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StyleAssignmentIdentityError {
    /// An opaque product reference was blank or numeric-like.
    InvalidReference,
    /// A digest or locale contained noncanonical content or was blank.
    NonCanonicalToken,
}

impl Display for StyleAssignmentIdentityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "style-assignment references must be opaque non-numeric values"
            }
            Self::NonCanonicalToken => {
                "style-assignment digests and locale must be nonblank canonical tokens"
            }
        })
    }
}

impl Error for StyleAssignmentIdentityError {}

impl StyleAssignmentIdentity<'_> {
    /// Serialize the deterministic assignment identity into the ADR-0018 canonical byte form.
    ///
    /// Fields are emitted in a fixed schema order. Each field name is followed by an unsigned
    /// 64-bit big-endian byte length and then the exact UTF-8 value. Opaque references are
    /// normalized with the product reference contract; digests remain exact tokens and locale
    /// must satisfy the fail-closed BCP 47 grammar used for published assessment locales.
    /// `norm_version_ref` additionally emits an explicit presence marker so `None` cannot be
    /// confused with any future present value.
    ///
    /// # Errors
    ///
    /// Returns [`StyleAssignmentIdentityError`] when an opaque reference is invalid or an exact
    /// token is blank/noncanonical.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StyleAssignmentIdentityError> {
        let (score_identity_kind, score_identity) = match self.score_identity {
            ScoreIdentity::ScoreProfileRef(reference) => {
                ("score_profile_ref", required_reference(reference)?)
            }
            ScoreIdentity::CanonicalScorePayloadDigest(digest) => (
                "canonical_score_payload_digest",
                required_exact_token(digest)?,
            ),
        };
        let instrument_version_ref = required_reference(self.instrument_version_ref)?;
        let scoring_version_ref = required_reference(self.scoring_version_ref)?;
        let style_mapping_version_ref = required_reference(self.style_mapping_version_ref)?;
        let interpretation_rule_bundle_digest =
            required_exact_token(self.interpretation_rule_bundle_digest)?;
        let locale = required_locale(self.locale)?;
        let norm_version_ref = self.norm_version_ref.map(required_reference).transpose()?;

        let mut canonical = Vec::with_capacity(384);
        canonical.extend_from_slice(STYLE_ASSIGNMENT_IDENTITY_DOMAIN);
        append_field(&mut canonical, "score_identity_kind", score_identity_kind);
        append_field(&mut canonical, "score_identity", score_identity);
        append_field(
            &mut canonical,
            "instrument_version_ref",
            instrument_version_ref,
        );
        append_field(&mut canonical, "scoring_version_ref", scoring_version_ref);
        append_field(
            &mut canonical,
            "norm_version_ref_present",
            if norm_version_ref.is_some() { "1" } else { "0" },
        );
        append_field(
            &mut canonical,
            "norm_version_ref",
            norm_version_ref.unwrap_or_default(),
        );
        append_field(
            &mut canonical,
            "style_mapping_version_ref",
            style_mapping_version_ref,
        );
        append_field(
            &mut canonical,
            "interpretation_rule_bundle_digest",
            interpretation_rule_bundle_digest,
        );
        append_field(&mut canonical, "locale", locale);
        Ok(canonical)
    }
}

fn required_reference(reference: &str) -> Result<&str, StyleAssignmentIdentityError> {
    normalized_reference(reference).ok_or(StyleAssignmentIdentityError::InvalidReference)
}

fn required_exact_token(token: &str) -> Result<&str, StyleAssignmentIdentityError> {
    if token.is_empty()
        || token.trim() != token
        || token.chars().any(char::is_control)
        || token.chars().any(char::is_whitespace)
    {
        Err(StyleAssignmentIdentityError::NonCanonicalToken)
    } else {
        Ok(token)
    }
}

fn required_locale(locale: &str) -> Result<&str, StyleAssignmentIdentityError> {
    let locale = required_exact_token(locale)?;
    if is_well_formed_bcp47(locale) {
        Ok(locale)
    } else {
        Err(StyleAssignmentIdentityError::NonCanonicalToken)
    }
}

fn is_well_formed_bcp47(locale: &str) -> bool {
    if GRANDFATHERED_LANGUAGE_TAGS
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case(locale))
    {
        return true;
    }

    let subtags: Vec<&str> = locale.split('-').collect();
    if subtags.iter().any(|subtag| subtag.is_empty()) {
        return false;
    }

    if subtags[0].eq_ignore_ascii_case("x") {
        return subtags.len() > 1 && subtags[1..].iter().all(|subtag| is_alnum_len(subtag, 1, 8));
    }

    let language = subtags[0];
    let language_allows_extlang = is_alpha_len(language, 2, 3);
    if !language_allows_extlang && !is_alpha_len(language, 4, 8) {
        return false;
    }

    let mut index = 1;
    if language_allows_extlang {
        let mut extlang_count = 0;
        while index < subtags.len() && extlang_count < 3 && is_alpha_len(subtags[index], 3, 3) {
            index += 1;
            extlang_count += 1;
        }
    }

    if index < subtags.len() && is_alpha_len(subtags[index], 4, 4) {
        index += 1;
    }

    if index < subtags.len() && is_region(subtags[index]) {
        index += 1;
    }

    let mut variants: Vec<&str> = Vec::new();
    while index < subtags.len() && is_variant(subtags[index]) {
        let variant = subtags[index];
        if variants
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(variant))
        {
            return false;
        }
        variants.push(variant);
        index += 1;
    }

    let mut extension_singletons: Vec<u8> = Vec::new();
    while index < subtags.len() && is_extension_singleton(subtags[index]) {
        let singleton = subtags[index].as_bytes()[0].to_ascii_lowercase();
        if extension_singletons.contains(&singleton) {
            return false;
        }
        extension_singletons.push(singleton);
        index += 1;

        let extension_start = index;
        while index < subtags.len()
            && !is_any_singleton(subtags[index])
            && is_alnum_len(subtags[index], 2, 8)
        {
            index += 1;
        }
        if index == extension_start {
            return false;
        }
    }

    if index < subtags.len() && subtags[index].eq_ignore_ascii_case("x") {
        index += 1;
        let private_use_start = index;
        while index < subtags.len() && is_alnum_len(subtags[index], 1, 8) {
            index += 1;
        }
        if index == private_use_start {
            return false;
        }
    }

    index == subtags.len()
}

fn is_alpha_len(subtag: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&subtag.len())
        && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_alnum_len(subtag: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&subtag.len())
        && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_region(subtag: &str) -> bool {
    is_alpha_len(subtag, 2, 2)
        || (subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_variant(subtag: &str) -> bool {
    is_alnum_len(subtag, 5, 8)
        || (subtag.len() == 4
            && subtag.as_bytes()[0].is_ascii_digit()
            && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

fn is_any_singleton(subtag: &str) -> bool {
    subtag.len() == 1 && subtag.as_bytes()[0].is_ascii_alphanumeric()
}

fn is_extension_singleton(subtag: &str) -> bool {
    is_any_singleton(subtag) && !subtag.eq_ignore_ascii_case("x")
}

fn append_field(target: &mut Vec<u8>, field_name: &str, value: &str) {
    let value_length = u64::try_from(value.len())
        .expect("Rust string lengths must fit the canonical unsigned 64-bit length field");
    target.extend_from_slice(field_name.as_bytes());
    target.extend_from_slice(&value_length.to_be_bytes());
    target.extend_from_slice(value.as_bytes());
}