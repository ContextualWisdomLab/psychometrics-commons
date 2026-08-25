//! Versioned Personality Style presentation mapping over already-scored Big Five evidence.
//!
//! This module does **not** estimate latent traits, calibrate items, or replace `fast-mlsirm`.
//! A caller supplies finite construct scores that a scoring engine already produced. The mapping
//! then chooses an original presentation style so a participant can read a memorable explanation
//! while the numeric profile remains the scientific source of truth.
//!
//! Version `style_mapping_version_v1` uses pole dominance: the largest expressed Big Five
//! dimension selects the primary style, and a close second dimension may appear as an adjacent
//! style. A dimension is expressed only when its absolute score is at least `0.50` and, when a
//! standard error is present, its absolute score is at least `1.96` times that standard error.
//! Those constants are presentation-policy parameters for this mapping version, not psychometric
//! cut scores or a claim that the resulting style is a latent class. Profiles with no expressed
//! dimension receive `style_balanced_profile` instead of a forced category.
//!
//! The style names are original product presentation labels. They are not official type scores
//! and must not be advertised as psychometric latent classes or MBTI equivalents. Continuous
//! scores remain the measurement source. Narrative text is separately versioned so this mapping
//! cannot hide uncertainty behind generic personality copy. Authoritative design constraints live
//! in `docs/adr/0018-continuous-scores-and-narrative-separation.md`.

use crate::deterministic_narrative::ApprovedStyleSelection;
use crate::narrative::{StyleAssignmentIdentity, StyleAssignmentKey};
use crate::scoring::{ObservationDisposition, ScoreObservation};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// First approved Personality Style mapping version.
pub const STYLE_MAPPING_VERSION_V1: &str = "style_mapping_version_v1";

const EXPRESSION_ABS_SCORE: f64 = 0.50;
const EXPRESSION_SE_MULTIPLIER: f64 = 1.96;
const ADJACENT_ABS_MARGIN: f64 = 0.25;

const CONSTRUCT_EXTRAVERSION: &str = "construct_extraversion";
const CONSTRUCT_AGREEABLENESS: &str = "construct_agreeableness";
const CONSTRUCT_CONSCIENTIOUSNESS: &str = "construct_conscientiousness";
const CONSTRUCT_NEUROTICISM: &str = "construct_neuroticism";
const CONSTRUCT_OPENNESS: &str = "construct_openness";

/// Deterministic presentation assignment produced from scored Big Five observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignedPersonalityStyle {
    /// Canonical ADR-0018 key bound to the supplied identity, not to optional AI wording.
    pub assignment_key: StyleAssignmentKey,
    /// Dominant original presentation style selected by this mapping version.
    pub primary_style_ref: &'static str,
    /// Close second styles preserved in descending absolute-score order.
    pub adjacent_style_refs: Vec<&'static str>,
    /// Interpretation-unit references for the primary style and then each adjacent style.
    pub interpretation_unit_refs: Vec<&'static str>,
}

impl AssignedPersonalityStyle {
    /// Borrow this assignment as the renderer input used by deterministic narrative fallback.
    #[must_use]
    pub fn as_approved_selection(&self) -> ApprovedStyleSelection<'_> {
        ApprovedStyleSelection {
            assignment_key: self.assignment_key,
            primary_style_ref: self.primary_style_ref,
            adjacent_style_refs: &self.adjacent_style_refs,
            interpretation_unit_refs: &self.interpretation_unit_refs,
        }
    }
}

/// Fail-closed error for the versioned Personality Style presentation mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StyleMappingError {
    /// The identity named a mapping version this runtime does not implement.
    UnsupportedMappingVersion,
    /// The supplied style-assignment identity was invalid or noncanonical.
    InvalidIdentity,
    /// A required Big Five construct was absent from the observation set.
    MissingRequiredConstruct,
    /// A required construct was present more than once.
    DuplicateConstruct,
    /// A required construct was not in the scored disposition with a finite score.
    UnscoredConstruct,
}

impl Display for StyleMappingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedMappingVersion => {
                "personality style mapping version is not supported"
            }
            Self::InvalidIdentity => "style-assignment identity is invalid",
            Self::MissingRequiredConstruct => {
                "personality style mapping requires all five scored Big Five constructs"
            }
            Self::DuplicateConstruct => {
                "personality style mapping rejects duplicate construct observations"
            }
            Self::UnscoredConstruct => {
                "personality style mapping requires a finite scored observation for each Big Five construct"
            }
        })
    }
}

impl Error for StyleMappingError {}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BigFiveConstruct {
    Extraversion,
    Agreeableness,
    Conscientiousness,
    Neuroticism,
    Openness,
}

const REQUIRED_DOMAINS: [BigFiveConstruct; 5] = [
    BigFiveConstruct::Extraversion,
    BigFiveConstruct::Agreeableness,
    BigFiveConstruct::Conscientiousness,
    BigFiveConstruct::Neuroticism,
    BigFiveConstruct::Openness,
];

impl BigFiveConstruct {
    const fn as_ref(self) -> &'static str {
        match self {
            Self::Extraversion => CONSTRUCT_EXTRAVERSION,
            Self::Agreeableness => CONSTRUCT_AGREEABLENESS,
            Self::Conscientiousness => CONSTRUCT_CONSCIENTIOUSNESS,
            Self::Neuroticism => CONSTRUCT_NEUROTICISM,
            Self::Openness => CONSTRUCT_OPENNESS,
        }
    }

    const fn pole(self, score: f64) -> (&'static str, &'static str) {
        let high = score >= 0.0;
        match self {
            Self::Extraversion => {
                if high {
                    ("style_social_engagement", "unit_social_engagement")
                } else {
                    ("style_reserved_focus", "unit_reserved_focus")
                }
            }
            Self::Agreeableness => {
                if high {
                    ("style_cooperative_regard", "unit_cooperative_regard")
                } else {
                    ("style_independent_challenge", "unit_independent_challenge")
                }
            }
            Self::Conscientiousness => {
                if high {
                    ("style_structured_pursuit", "unit_structured_pursuit")
                } else {
                    ("style_flexible_adaptation", "unit_flexible_adaptation")
                }
            }
            Self::Neuroticism => {
                if high {
                    ("style_affective_sensitivity", "unit_affective_sensitivity")
                } else {
                    ("style_even_affect", "unit_even_affect")
                }
            }
            Self::Openness => {
                if high {
                    ("style_exploratory_openness", "unit_exploratory_openness")
                } else {
                    (
                        "style_conventional_grounding",
                        "unit_conventional_grounding",
                    )
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ExpressedPole {
    abs_score: f64,
    construct: BigFiveConstruct,
    style_ref: &'static str,
    unit_ref: &'static str,
}

/// Assign an original Personality Style from already-scored Big Five observations.
///
/// Callers must pass the same [`StyleAssignmentIdentity`] they will later give the deterministic
/// narrative renderer. This function reads scores only; it never writes back into `observations`
/// and never changes numeric score values. The thresholds are versioned presentation policy, not
/// scientific score thresholds, diagnostic boundaries, or latent-class estimates.
///
/// # Errors
///
/// Returns [`StyleMappingError`] when the mapping version is unknown, the identity is invalid,
/// a required construct is missing or duplicated, or a required construct is not scored.
pub fn assign_personality_style(
    identity: &StyleAssignmentIdentity<'_>,
    observations: &[ScoreObservation],
) -> Result<AssignedPersonalityStyle, StyleMappingError> {
    if identity.style_mapping_version_ref != STYLE_MAPPING_VERSION_V1 {
        return Err(StyleMappingError::UnsupportedMappingVersion);
    }
    let assignment_key = identity
        .assignment_key()
        .map_err(|_| StyleMappingError::InvalidIdentity)?;

    let mut expressed = Vec::with_capacity(REQUIRED_DOMAINS.len());
    for construct in REQUIRED_DOMAINS {
        let (observation, score) = required_scored_observation(observations, construct.as_ref())?;
        if !is_expressed(score, observation.standard_error()) {
            continue;
        }
        let (style_ref, unit_ref) = construct.pole(score);
        expressed.push(ExpressedPole {
            abs_score: score.abs(),
            construct,
            style_ref,
            unit_ref,
        });
    }

    expressed.sort_by(|left, right| {
        right
            .abs_score
            .total_cmp(&left.abs_score)
            .then(left.construct.as_ref().cmp(right.construct.as_ref()))
    });

    if expressed.is_empty() {
        return Ok(AssignedPersonalityStyle {
            assignment_key,
            primary_style_ref: "style_balanced_profile",
            adjacent_style_refs: Vec::new(),
            interpretation_unit_refs: vec!["unit_balanced_profile"],
        });
    }

    let primary = expressed[0];
    let adjacent: Vec<ExpressedPole> = expressed
        .iter()
        .skip(1)
        .copied()
        .filter(|pole| primary.abs_score - pole.abs_score <= ADJACENT_ABS_MARGIN)
        .collect();

    let mut interpretation_unit_refs = Vec::with_capacity(1 + adjacent.len());
    interpretation_unit_refs.push(primary.unit_ref);
    interpretation_unit_refs.extend(adjacent.iter().map(|pole| pole.unit_ref));

    Ok(AssignedPersonalityStyle {
        assignment_key,
        primary_style_ref: primary.style_ref,
        adjacent_style_refs: adjacent.iter().map(|pole| pole.style_ref).collect(),
        interpretation_unit_refs,
    })
}

fn required_scored_observation<'a>(
    observations: &'a [ScoreObservation],
    construct_ref: &str,
) -> Result<(&'a ScoreObservation, f64), StyleMappingError> {
    let mut found = None;
    for observation in observations {
        if observation.construct_ref() != construct_ref {
            continue;
        }
        if found.is_some() {
            return Err(StyleMappingError::DuplicateConstruct);
        }
        found = Some(observation);
    }
    let observation = found.ok_or(StyleMappingError::MissingRequiredConstruct)?;
    match (observation.disposition(), observation.score()) {
        (ObservationDisposition::Scored, Some(score)) => Ok((observation, score)),
        (
            ObservationDisposition::Abstained
            | ObservationDisposition::Failed
            | ObservationDisposition::Excluded
            | ObservationDisposition::Scored,
            _,
        ) => Err(StyleMappingError::UnscoredConstruct),
    }
}

fn is_expressed(score: f64, standard_error: Option<f64>) -> bool {
    let abs_score = score.abs();
    if abs_score < EXPRESSION_ABS_SCORE {
        return false;
    }
    match standard_error {
        None => true,
        Some(error) => abs_score >= EXPRESSION_SE_MULTIPLIER * error,
    }
}
