//! Restricted mapping from an operational participant to a research pseudonym.
//!
//! Authorized research contribution needs this mapping. Public research
//! releases must not. The product therefore stores the linkage as its own
//! purpose-bound record and projects only the research identity into a release
//! fixture. This module does not mint cryptographic keys and does not mask
//! construct-relevant operational data used by authorized assessment work.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Fail-closed construction error for one restricted identity linkage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RestrictedIdentityLinkageError {
    /// A linkage, participant, program, or key-version reference was blank, padded, or numeric-like.
    InvalidReference,
    /// The research identity reused the operational participant reference.
    OperationalIdentityReuse,
    /// The platform recorded-time was missing.
    InvalidRecordedTime,
}

impl Display for RestrictedIdentityLinkageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "restricted linkage references must be unpadded opaque values"
            }
            Self::OperationalIdentityReuse => {
                "research identity must not reuse the operational participant"
            }
            Self::InvalidRecordedTime => {
                "restricted linkage recorded time must be a positive Unix millisecond instant"
            }
        })
    }
}

impl Error for RestrictedIdentityLinkageError {}

/// Immutable restricted mapping used only by authorized research workflows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestrictedIdentityLinkage {
    linkage_ref: String,
    participant_ref: String,
    research_participant_ref: String,
    research_program_ref: String,
    linkage_key_version: String,
    recorded_at_unix_ms: u64,
}

impl RestrictedIdentityLinkage {
    /// Bind one operational participant to one program-scoped research identity.
    ///
    /// The same operational participant may hold a different research identity
    /// in another program. Those identities are not collapsed. The linkage-key
    /// version names the key-management generation; it is not the secret key.
    ///
    /// # Errors
    ///
    /// Returns [`RestrictedIdentityLinkageError::InvalidReference`] when any
    /// identity is blank, whitespace-padded, or numeric-like,
    /// [`RestrictedIdentityLinkageError::OperationalIdentityReuse`] when the
    /// research identity equals the operational participant, or
    /// [`RestrictedIdentityLinkageError::InvalidRecordedTime`] when the
    /// platform instant is zero.
    pub fn new(
        linkage_ref: &str,
        participant_ref: &str,
        research_participant_ref: &str,
        research_program_ref: &str,
        linkage_key_version: &str,
        recorded_at_unix_ms: u64,
    ) -> Result<Self, RestrictedIdentityLinkageError> {
        let linkage_ref = exact_reference(linkage_ref)?;
        let participant_ref = exact_reference(participant_ref)?;
        let research_participant_ref = exact_reference(research_participant_ref)?;
        let research_program_ref = exact_reference(research_program_ref)?;
        let linkage_key_version = exact_reference(linkage_key_version)?;
        if research_participant_ref == participant_ref
            || research_program_ref == participant_ref
            || research_participant_ref == research_program_ref
        {
            return Err(RestrictedIdentityLinkageError::OperationalIdentityReuse);
        }
        if recorded_at_unix_ms == 0 {
            return Err(RestrictedIdentityLinkageError::InvalidRecordedTime);
        }
        Ok(Self {
            linkage_ref: linkage_ref.to_owned(),
            participant_ref: participant_ref.to_owned(),
            research_participant_ref: research_participant_ref.to_owned(),
            research_program_ref: research_program_ref.to_owned(),
            linkage_key_version: linkage_key_version.to_owned(),
            recorded_at_unix_ms,
        })
    }

    /// Return the opaque restricted-linkage identity.
    #[must_use]
    pub fn linkage_ref(&self) -> &str {
        &self.linkage_ref
    }

    /// Return the operational participant. Authorized research work may use this.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the program-scoped research pseudonym.
    #[must_use]
    pub fn research_participant_ref(&self) -> &str {
        &self.research_participant_ref
    }

    /// Return the research program that scopes this pseudonym.
    #[must_use]
    pub fn research_program_ref(&self) -> &str {
        &self.research_program_ref
    }

    /// Return the linkage-key generation, never the secret key material.
    #[must_use]
    pub fn linkage_key_version(&self) -> &str {
        &self.linkage_key_version
    }

    /// Return the platform instant when this linkage was recorded.
    #[must_use]
    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }

    /// Project only the research identity that a public release may carry.
    #[must_use]
    pub fn public_release_projection(&self) -> PublicResearchReleaseProjection {
        PublicResearchReleaseProjection {
            research_participant_ref: self.research_participant_ref.clone(),
            research_program_ref: self.research_program_ref.clone(),
        }
    }
}

/// Public-release view that cannot carry operational or linkage-key identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicResearchReleaseProjection {
    research_participant_ref: String,
    research_program_ref: String,
}

impl PublicResearchReleaseProjection {
    /// Build a public-release identity from research identifiers only.
    ///
    /// A release fixture calls this after reading `public_research_identity`.
    /// It cannot carry an operational participant or linkage-key version.
    ///
    /// # Errors
    ///
    /// Returns [`RestrictedIdentityLinkageError::InvalidReference`] when either
    /// identity is blank, whitespace-padded, or numeric-like, or
    /// [`RestrictedIdentityLinkageError::OperationalIdentityReuse`] when the
    /// research identity equals the program.
    pub fn new(
        research_participant_ref: &str,
        research_program_ref: &str,
    ) -> Result<Self, RestrictedIdentityLinkageError> {
        let research_participant_ref = exact_reference(research_participant_ref)?;
        let research_program_ref = exact_reference(research_program_ref)?;
        if research_participant_ref == research_program_ref {
            return Err(RestrictedIdentityLinkageError::OperationalIdentityReuse);
        }
        Ok(Self {
            research_participant_ref: research_participant_ref.to_owned(),
            research_program_ref: research_program_ref.to_owned(),
        })
    }

    /// Return the research pseudonym allowed in a public release fixture.
    #[must_use]
    pub fn research_participant_ref(&self) -> &str {
        &self.research_participant_ref
    }

    /// Return the research program that scopes the public pseudonym.
    #[must_use]
    pub fn research_program_ref(&self) -> &str {
        &self.research_program_ref
    }
}

fn exact_reference(reference: &str) -> Result<&str, RestrictedIdentityLinkageError> {
    if reference.trim() != reference {
        return Err(RestrictedIdentityLinkageError::InvalidReference);
    }
    normalized_reference(reference).ok_or(RestrictedIdentityLinkageError::InvalidReference)
}

#[cfg(test)]
mod tests {
    use super::{
        exact_reference, PublicResearchReleaseProjection, RestrictedIdentityLinkage,
        RestrictedIdentityLinkageError,
    };

    #[test]
    fn program_or_research_identity_cannot_reuse_another_namespace() {
        let program_collision = RestrictedIdentityLinkage::new(
            "linkage_commons_program_one",
            "participant_operational_one",
            "research_program_commons_one",
            "research_program_commons_one",
            "linkage_key_version_2026_q3",
            1_724_000_000_000,
        )
        .expect_err("research participant must not equal the program");
        assert_eq!(
            program_collision,
            RestrictedIdentityLinkageError::OperationalIdentityReuse
        );

        let operational_program = RestrictedIdentityLinkage::new(
            "linkage_commons_program_one",
            "participant_operational_one",
            "research_participant_program_one",
            "participant_operational_one",
            "linkage_key_version_2026_q3",
            1_724_000_000_000,
        )
        .expect_err("research program must not equal the operational participant");
        assert_eq!(
            operational_program,
            RestrictedIdentityLinkageError::OperationalIdentityReuse
        );
    }

    #[test]
    fn exact_reference_and_accessors_cover_valid_linkage() {
        assert!(matches!(
            exact_reference(" padded"),
            Err(RestrictedIdentityLinkageError::InvalidReference)
        ));
        assert_eq!(exact_reference("linkage_one").unwrap(), "linkage_one");
        let linkage = RestrictedIdentityLinkage::new(
            "linkage_commons_program_one",
            "participant_operational_one",
            "research_participant_program_one",
            "research_program_commons_one",
            "linkage_key_version_2026_q3",
            1_724_000_000_000,
        )
        .unwrap();
        assert_eq!(linkage.linkage_ref(), "linkage_commons_program_one");
        assert_eq!(linkage.recorded_at_unix_ms(), 1_724_000_000_000);
        assert_eq!(
            linkage
                .public_release_projection()
                .research_participant_ref(),
            linkage.research_participant_ref()
        );
        let public = PublicResearchReleaseProjection::new(
            linkage.research_participant_ref(),
            linkage.research_program_ref(),
        )
        .unwrap();
        assert_eq!(
            public.research_program_ref(),
            linkage.research_program_ref()
        );
        assert!(matches!(
            PublicResearchReleaseProjection::new(
                linkage.research_participant_ref(),
                linkage.research_participant_ref()
            ),
            Err(RestrictedIdentityLinkageError::OperationalIdentityReuse)
        ));
    }
}
