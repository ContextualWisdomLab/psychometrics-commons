//! Deterministic, AI-independent narrative fallback for approved Personality Style evidence.
//!
//! This module is intentionally downstream of scientific scoring and style assignment. It does
//! not calculate Big Five scores, choose a Personality Style, infer a construct, or call an LLM.
//! Instead, it verifies the ADR-0018 canonical style-assignment identity and renders only the
//! localized interpretation units selected by an approved, separately versioned mapping.

use crate::narrative::{StyleAssignmentIdentity, StyleAssignmentKey};
use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// One approved localized interpretation unit in a deterministic narrative bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NarrativeUnit<'a> {
    /// Opaque versioned reference used by the mapping to select this interpretation.
    pub interpretation_unit_ref: &'a str,
    /// Participant-facing section heading for the exact bundle locale.
    pub heading: &'a str,
    /// Participant-facing deterministic body text for the exact bundle locale.
    pub body: &'a str,
}

/// Exact deterministic style-selection evidence produced by an approved mapping.
///
/// The selection is presentation evidence only. It deliberately carries no mutable score fields
/// and cannot replace the underlying continuous/facet score profile or uncertainty evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApprovedStyleSelection<'a> {
    /// Canonical ADR-0018 key derived from immutable score and mapping provenance.
    pub assignment_key: StyleAssignmentKey,
    /// Primary presentation style reference selected by the approved mapping.
    pub primary_style_ref: &'a str,
    /// Optional adjacent/mixed style references selected by the approved mapping.
    pub adjacent_style_refs: &'a [&'a str],
    /// Ordered interpretation-unit references selected by the approved mapping.
    pub interpretation_unit_refs: &'a [&'a str],
}

/// Versioned deterministic localized narrative bundle.
///
/// Bundle provenance must exactly agree with the canonical style-assignment identity. A bundle
/// cannot silently reinterpret a selection from another mapping, rule digest, or locale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeterministicNarrativeBundle<'a> {
    /// Immutable narrative content version.
    pub narrative_version_ref: &'a str,
    /// Deterministic style-mapping version supported by this bundle.
    pub style_mapping_version_ref: &'a str,
    /// SHA-256 digest of the approved interpretation-rule bundle.
    pub interpretation_rule_bundle_digest: &'a str,
    /// Exact locale of every participant-facing string in this bundle.
    pub locale: &'a str,
    /// Approved localized interpretation units available to the renderer.
    pub units: &'a [NarrativeUnit<'a>],
    /// Participant-facing limitations that remain visible with every rendering.
    pub limitations: &'a [&'a str],
}

/// One rendered deterministic interpretation section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedNarrativeSection {
    /// Opaque interpretation-unit reference that justified the section.
    pub interpretation_unit_ref: String,
    /// Localized section heading.
    pub heading: String,
    /// Localized deterministic section text.
    pub body: String,
}

/// AI-independent result-presentation artifact produced from verified approved evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedNarrative {
    /// Immutable narrative content version used for this rendering.
    pub narrative_version_ref: String,
    /// Exact locale used for this rendering.
    pub locale: String,
    /// Canonical style-assignment key bound to immutable score/mapping provenance.
    pub assignment_key: StyleAssignmentKey,
    /// Primary presentation style reference from the approved selection.
    pub primary_style_ref: String,
    /// Optional adjacent/mixed styles preserved in selection order.
    pub adjacent_style_refs: Vec<String>,
    /// Deterministic localized interpretation sections preserved in selection order.
    pub sections: Vec<RenderedNarrativeSection>,
    /// Required participant-facing limitations from the approved bundle.
    pub limitations: Vec<String>,
}

/// Fail-closed deterministic narrative validation or rendering error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NarrativeFallbackError {
    /// An opaque product reference was blank or numeric-like.
    InvalidReference,
    /// Participant-facing approved text was blank or noncanonical.
    InvalidText,
    /// The interpretation-rule digest was not a canonical lowercase SHA-256 token.
    InvalidDigest,
    /// The supplied style-assignment identity itself was invalid.
    InvalidIdentity,
    /// Bundle or selection provenance did not match the canonical assignment identity.
    IdentityMismatch,
    /// A style or interpretation reference appeared more than once where uniqueness is required.
    DuplicateReference,
    /// The approved selection contained no interpretation units.
    EmptySelection,
    /// A selected interpretation unit did not exist in the approved localized bundle.
    MissingInterpretationUnit,
}

impl Display for NarrativeFallbackError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => "narrative references must be opaque non-numeric values",
            Self::InvalidText => "narrative text must be nonblank canonical display text",
            Self::InvalidDigest => "narrative rule digest must be canonical lowercase SHA-256",
            Self::InvalidIdentity => "style-assignment identity is invalid",
            Self::IdentityMismatch => "narrative provenance does not match style assignment",
            Self::DuplicateReference => "narrative style or interpretation reference is duplicated",
            Self::EmptySelection => "narrative selection must contain interpretation units",
            Self::MissingInterpretationUnit => {
                "selected interpretation unit is absent from the approved bundle"
            }
        })
    }
}

impl Error for NarrativeFallbackError {}

impl DeterministicNarrativeBundle<'_> {
    /// Render the approved deterministic fallback for one canonical style assignment.
    ///
    /// This operation verifies the canonical assignment key, mapping version, rule digest, and
    /// locale before it selects any participant-facing text. Interpretation units are emitted in
    /// the mapping-provided order; missing or duplicated references fail closed. No score value is
    /// accepted by this API, so rendering cannot mutate or substitute scientific score evidence.
    ///
    /// # Errors
    ///
    /// Returns [`NarrativeFallbackError`] when identity, provenance, references, bundle text, or
    /// interpretation-unit coverage is invalid or inconsistent.
    pub fn render(
        &self,
        identity: &StyleAssignmentIdentity<'_>,
        selection: &ApprovedStyleSelection<'_>,
    ) -> Result<RenderedNarrative, NarrativeFallbackError> {
        self.validate()?;
        let assignment_key = identity
            .assignment_key()
            .map_err(|_| NarrativeFallbackError::InvalidIdentity)?;
        if assignment_key != selection.assignment_key {
            return Err(NarrativeFallbackError::IdentityMismatch);
        }

        let identity_mapping = required_reference(identity.style_mapping_version_ref)?;
        let bundle_mapping = required_reference(self.style_mapping_version_ref)?;
        if identity_mapping != bundle_mapping
            || identity.interpretation_rule_bundle_digest != self.interpretation_rule_bundle_digest
            || identity.locale != self.locale
        {
            return Err(NarrativeFallbackError::IdentityMismatch);
        }

        let primary_style_ref = required_reference(selection.primary_style_ref)?;
        validate_adjacent_styles(primary_style_ref, selection.adjacent_style_refs)?;
        validate_interpretation_selection(selection.interpretation_unit_refs)?;

        let mut sections = Vec::with_capacity(selection.interpretation_unit_refs.len());
        for selected_ref in selection.interpretation_unit_refs {
            let selected_ref = required_reference(selected_ref)?;
            let unit = self
                .units
                .iter()
                .find(|unit| {
                    normalized_reference(unit.interpretation_unit_ref) == Some(selected_ref)
                })
                .ok_or(NarrativeFallbackError::MissingInterpretationUnit)?;
            sections.push(RenderedNarrativeSection {
                interpretation_unit_ref: selected_ref.to_owned(),
                heading: unit.heading.to_owned(),
                body: unit.body.to_owned(),
            });
        }

        Ok(RenderedNarrative {
            narrative_version_ref: required_reference(self.narrative_version_ref)?.to_owned(),
            locale: self.locale.to_owned(),
            assignment_key,
            primary_style_ref: primary_style_ref.to_owned(),
            adjacent_style_refs: selection
                .adjacent_style_refs
                .iter()
                .map(|reference| required_reference(reference).map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?,
            sections,
            limitations: self.limitations.iter().map(|text| (*text).to_owned()).collect(),
        })
    }

    fn validate(&self) -> Result<(), NarrativeFallbackError> {
        required_reference(self.narrative_version_ref)?;
        required_reference(self.style_mapping_version_ref)?;
        required_digest(self.interpretation_rule_bundle_digest)?;
        required_text(self.locale)?;

        let mut unit_refs: Vec<&str> = Vec::with_capacity(self.units.len());
        for unit in self.units {
            let unit_ref = required_reference(unit.interpretation_unit_ref)?;
            if unit_refs.contains(&unit_ref) {
                return Err(NarrativeFallbackError::DuplicateReference);
            }
            unit_refs.push(unit_ref);
            required_text(unit.heading)?;
            required_text(unit.body)?;
        }
        for limitation in self.limitations {
            required_text(limitation)?;
        }
        Ok(())
    }
}

fn validate_adjacent_styles(
    primary_style_ref: &str,
    adjacent_style_refs: &[&str],
) -> Result<(), NarrativeFallbackError> {
    let mut seen = vec![primary_style_ref];
    for adjacent in adjacent_style_refs {
        let adjacent = required_reference(adjacent)?;
        if seen.contains(&adjacent) {
            return Err(NarrativeFallbackError::DuplicateReference);
        }
        seen.push(adjacent);
    }
    Ok(())
}

fn validate_interpretation_selection(
    interpretation_unit_refs: &[&str],
) -> Result<(), NarrativeFallbackError> {
    if interpretation_unit_refs.is_empty() {
        return Err(NarrativeFallbackError::EmptySelection);
    }
    let mut seen = Vec::with_capacity(interpretation_unit_refs.len());
    for unit_ref in interpretation_unit_refs {
        let unit_ref = required_reference(unit_ref)?;
        if seen.contains(&unit_ref) {
            return Err(NarrativeFallbackError::DuplicateReference);
        }
        seen.push(unit_ref);
    }
    Ok(())
}

fn required_reference(reference: &str) -> Result<&str, NarrativeFallbackError> {
    normalized_reference(reference).ok_or(NarrativeFallbackError::InvalidReference)
}

fn required_text(text: &str) -> Result<&str, NarrativeFallbackError> {
    if text.trim().is_empty() || text.trim() != text || text.chars().any(char::is_control) {
        Err(NarrativeFallbackError::InvalidText)
    } else {
        Ok(text)
    }
}

fn required_digest(digest: &str) -> Result<&str, NarrativeFallbackError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(NarrativeFallbackError::InvalidDigest);
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(digest)
    } else {
        Err(NarrativeFallbackError::InvalidDigest)
    }
}
