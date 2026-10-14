//! Deterministic presentation identity primitives for Personality Style narrative.
//!
//! Personality Style is a presentation mapping over pinned scientific score evidence. This
//! module does not calculate psychometric scores and does not let model/provider identity
//! alter deterministic style assignment. It defines the canonical behavior-affecting identity
//! and SHA-256 assignment key that ADR-0018 requires before persistence or public APIs.

use crate::reference::canonical_opaque_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

const STYLE_ASSIGNMENT_IDENTITY_DOMAIN: &[u8] =
    b"psychometrics-commons/style-assignment-identity/v1\0";

const SHA256_INITIAL_STATE: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

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

/// Stable SHA-256 identifier for one deterministic style assignment.
///
/// “Canonical” means the same valid behavior-affecting inputs always produce the same key.
/// The key is “opaque”: callers should compare or store its bytes, not assign meaning to
/// individual bytes. Its SHA-256 digest is the fixed 32-byte result derived from the validated
/// canonical assignment identity. Invalid identity input is rejected before any key is produced.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StyleAssignmentKey([u8; 32]);

impl StyleAssignmentKey {
    /// Return the 32-byte SHA-256 result for storage or equality checks.
    ///
    /// These bytes are opaque product identity data: do not interpret individual bytes as fields
    /// or scientific meaning. Identical valid canonical inputs produce identical bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
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
                "style-assignment references must be exact opaque non-numeric values without surrounding whitespace or unsafe control characters"
            }
            Self::NonCanonicalToken => {
                "style-assignment digests and locale must be nonblank canonical tokens"
            }
        })
    }
}

impl Error for StyleAssignmentIdentityError {}

impl StyleAssignmentIdentity<'_> {
    /// Compute the ADR-0018 SHA-256 key for this canonical deterministic assignment identity.
    ///
    /// The method first validates and canonicalizes every behavior-affecting input. If validation
    /// fails, it returns an error and does not produce a key. For valid input, the SHA-256 digest
    /// is a deterministic product identity, not an authentication primitive or a replacement for
    /// tenant/resource authorization.
    ///
    /// # Errors
    ///
    /// Returns [`StyleAssignmentIdentityError`] when any behavior-affecting identity input is
    /// invalid or noncanonical.
    pub fn assignment_key(&self) -> Result<StyleAssignmentKey, StyleAssignmentIdentityError> {
        Ok(StyleAssignmentKey(sha256(&self.canonical_bytes()?)))
    }

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
                required_sha256_digest(digest)?,
            ),
        };
        let instrument_version_ref = required_reference(self.instrument_version_ref)?;
        let scoring_version_ref = required_reference(self.scoring_version_ref)?;
        let style_mapping_version_ref = required_reference(self.style_mapping_version_ref)?;
        let interpretation_rule_bundle_digest =
            required_sha256_digest(self.interpretation_rule_bundle_digest)?;
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
    canonical_opaque_reference(reference).ok_or(StyleAssignmentIdentityError::InvalidReference)
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

fn required_sha256_digest(digest: &str) -> Result<&str, StyleAssignmentIdentityError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(StyleAssignmentIdentityError::NonCanonicalToken);
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(digest)
    } else {
        Err(StyleAssignmentIdentityError::NonCanonicalToken)
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

fn sha256(message: &[u8]) -> [u8; 32] {
    let message_length = u64::try_from(message.len())
        .expect("Rust slice lengths must fit the SHA-256 unsigned 64-bit length field");
    let bit_length = message_length
        .checked_mul(8)
        .expect("SHA-256 input must fit its unsigned 64-bit bit-length field");
    let zero_padding = (55 + 64 - (message.len() % 64)) % 64;

    let mut padded = Vec::with_capacity(message.len() + 1 + zero_padding + 8);
    padded.extend_from_slice(message);
    padded.push(0x80);
    padded.extend(std::iter::repeat_n(0, zero_padding));
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut state = SHA256_INITIAL_STATE;
    for chunk in padded.chunks_exact(64) {
        let mut schedule = [0_u32; 64];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }

        let mut schedule_index = 16;
        while schedule_index < 64 {
            let x = schedule[schedule_index - 15];
            let y = schedule[schedule_index - 2];
            let sigma0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let sigma1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            schedule[schedule_index] = schedule[schedule_index - 16]
                .wrapping_add(sigma0)
                .wrapping_add(schedule[schedule_index - 7])
                .wrapping_add(sigma1);
            schedule_index += 1;
        }

        let [mut working_a, mut working_b, mut working_c, mut working_d, mut working_e, mut working_f, mut working_g, mut working_h] =
            state;
        for (&round_constant, &word) in SHA256_ROUND_CONSTANTS.iter().zip(schedule.iter()) {
            let sum1 =
                working_e.rotate_right(6) ^ working_e.rotate_right(11) ^ working_e.rotate_right(25);
            let choose = (working_e & working_f) ^ ((!working_e) & working_g);
            let temporary1 = working_h
                .wrapping_add(sum1)
                .wrapping_add(choose)
                .wrapping_add(round_constant)
                .wrapping_add(word);
            let sum0 =
                working_a.rotate_right(2) ^ working_a.rotate_right(13) ^ working_a.rotate_right(22);
            let majority =
                (working_a & working_b) ^ (working_a & working_c) ^ (working_b & working_c);
            let temporary2 = sum0.wrapping_add(majority);

            working_h = working_g;
            working_g = working_f;
            working_f = working_e;
            working_e = working_d.wrapping_add(temporary1);
            working_d = working_c;
            working_c = working_b;
            working_b = working_a;
            working_a = temporary1.wrapping_add(temporary2);
        }

        state[0] = state[0].wrapping_add(working_a);
        state[1] = state[1].wrapping_add(working_b);
        state[2] = state[2].wrapping_add(working_c);
        state[3] = state[3].wrapping_add(working_d);
        state[4] = state[4].wrapping_add(working_e);
        state[5] = state[5].wrapping_add(working_f);
        state[6] = state[6].wrapping_add(working_g);
        state[7] = state[7].wrapping_add(working_h);
    }

    let mut digest = [0_u8; 32];
    for (index, word) in state.iter().enumerate() {
        let offset = index * 4;
        digest[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::sha256;

    #[test]
    fn sha256_matches_nist_abc_vector() {
        assert_eq!(
            sha256(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }
}
