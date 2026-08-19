//! Versioned Quick and Deep assessment delivery paths.
//!
//! An assessment path chooses which already-published item versions a participant receives for one
//! immutable instrument release. This module validates provenance and preserves the release's item
//! order. It does not choose items psychometrically, calculate scores, or replace evidence owned by
//! the published release or `fast-mlsirm`.

use crate::instrument::InstrumentReleaseManifest;
use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Participant-facing depth of one assessment delivery path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AssessmentPath {
    /// A shorter approved delivery path.
    Quick,
    /// A more comprehensive approved delivery path.
    Deep,
}

/// Fail-closed error returned while binding an assessment path to a release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AssessmentPathError {
    /// The path-policy version reference is blank, numeric-like, unsafe, or not exact.
    InvalidReference,
    /// The path contains no item versions.
    EmptyItemSet,
    /// The path repeats an item-version reference.
    DuplicateItemReference,
    /// The path names an item version that is not part of the immutable release.
    ItemOutsideRelease,
    /// The path changes the semantically significant item order of the immutable release.
    ItemOrderMismatch,
}

impl Display for AssessmentPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "assessment path policy reference must be an exact opaque non-numeric value"
            }
            Self::EmptyItemSet => "assessment path must contain at least one item version",
            Self::DuplicateItemReference => {
                "assessment path item-version references must be unique"
            }
            Self::ItemOutsideRelease => {
                "assessment path items must belong to the exact immutable instrument release"
            }
            Self::ItemOrderMismatch => {
                "assessment path items must preserve the immutable instrument release order"
            }
        })
    }
}

impl Error for AssessmentPathError {}

/// Immutable product evidence for one versioned assessment delivery path.
///
/// The definition copies only identity and ordered item references from the supplied immutable
/// release. A Quick or Deep label therefore cannot silently rebind to another release version or
/// reorder its items. The policy-version reference identifies the reviewed product rule that chose
/// this subset; scientific item-selection and scoring remain outside this module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssessmentPathDefinition {
    path: AssessmentPath,
    policy_version_ref: String,
    release_ref: String,
    instrument_version_ref: String,
    locale: String,
    item_version_refs: Vec<String>,
}

impl AssessmentPathDefinition {
    /// Bind a Quick or Deep path to an ordered subset of one immutable instrument release.
    ///
    /// Every path item must already belong to the release and appear in the same relative order as
    /// the release. This boundary does not decide how many items a Quick or Deep path should use;
    /// that choice must already be represented by a reviewed, versioned policy.
    ///
    /// # Errors
    ///
    /// Returns [`AssessmentPathError::InvalidReference`] when `policy_version_ref` is not an exact
    /// opaque reference, [`AssessmentPathError::EmptyItemSet`] for an empty path,
    /// [`AssessmentPathError::DuplicateItemReference`] for a repeated item,
    /// [`AssessmentPathError::ItemOutsideRelease`] for an item absent from the release, or
    /// [`AssessmentPathError::ItemOrderMismatch`] when the subset reorders release items.
    pub fn new(
        path: AssessmentPath,
        policy_version_ref: &str,
        release: &InstrumentReleaseManifest,
        item_version_refs: &[&str],
    ) -> Result<Self, AssessmentPathError> {
        let Some(normalized_policy_ref) = normalized_reference(policy_version_ref) else {
            return Err(AssessmentPathError::InvalidReference);
        };
        if normalized_policy_ref != policy_version_ref {
            return Err(AssessmentPathError::InvalidReference);
        }
        if item_version_refs.is_empty() {
            return Err(AssessmentPathError::EmptyItemSet);
        }

        let release_items = release.item_version_refs();
        let mut accepted = Vec::with_capacity(item_version_refs.len());
        let mut previous_position = None;
        for item_ref in item_version_refs {
            if accepted.iter().any(|accepted_ref| accepted_ref == item_ref) {
                return Err(AssessmentPathError::DuplicateItemReference);
            }
            let Some(position) = release_items.iter().position(|candidate| candidate == item_ref)
            else {
                return Err(AssessmentPathError::ItemOutsideRelease);
            };
            if previous_position.is_some_and(|previous| position <= previous) {
                return Err(AssessmentPathError::ItemOrderMismatch);
            }
            accepted.push((*item_ref).to_owned());
            previous_position = Some(position);
        }

        Ok(Self {
            path,
            policy_version_ref: policy_version_ref.to_owned(),
            release_ref: release.release_ref().to_owned(),
            instrument_version_ref: release.instrument_version_ref().to_owned(),
            locale: release.locale().to_owned(),
            item_version_refs: accepted,
        })
    }

    /// Return whether this definition is the Quick or Deep path.
    #[must_use]
    pub const fn path(&self) -> AssessmentPath {
        self.path
    }

    /// Return the reviewed product policy version that selected this path.
    #[must_use]
    pub fn policy_version_ref(&self) -> &str {
        &self.policy_version_ref
    }

    /// Return the immutable release identity this path belongs to.
    #[must_use]
    pub fn release_ref(&self) -> &str {
        &self.release_ref
    }

    /// Return the exact instrument-version identity copied from the release.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the exact locale copied from the immutable release.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the ordered item versions delivered by this path.
    #[must_use]
    pub fn item_version_refs(&self) -> &[String] {
        &self.item_version_refs
    }
}
