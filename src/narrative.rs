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
    /// must satisfy the fail-closed BCP 47 subtag shape used for published assessment locales.
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
    let (language, remainder) = match locale.split_once('-') {
        Some((language, remainder)) => (language, Some(remainder)),
        None => (locale, None),
    };
    if !(2..=8).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return Err(StyleAssignmentIdentityError::NonCanonicalToken);
    }
    if remainder.is_some_and(|subtags| {
        subtags.split('-').any(|subtag| {
            subtag.is_empty()
                || subtag.len() > 8
                || !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    }) {
        return Err(StyleAssignmentIdentityError::NonCanonicalToken);
    }
    Ok(locale)
}

fn append_field(target: &mut Vec<u8>, field_name: &str, value: &str) {
    let value_length = u64::try_from(value.len())
        .expect("Rust string lengths must fit the canonical unsigned 64-bit length field");
    target.extend_from_slice(field_name.as_bytes());
    target.extend_from_slice(&value_length.to_be_bytes());
    target.extend_from_slice(value.as_bytes());
}
