//! Product-owned tenant and resource authorization primitives.
//!
//! Identity proof and federation remain owned by Keyverse. Psychometrics Commons
//! consumes authenticated identity claims and makes its own domain authorization
//! decisions. Tenant context is server-derived, cross-tenant access fails closed,
//! and research approval remains separated from instrument publication and tenant
//! administration.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Product-domain role used for privileged Psychometrics Commons operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProductRole {
    /// Participant role carried for explicit product-domain audit context.
    Participant,
    /// May publish or govern instrument releases within the authenticated tenant.
    InstrumentPublisher,
    /// May approve research releases within the authenticated tenant.
    ResearchSteward,
    /// May administer tenant-scoped product configuration.
    TenantAdministrator,
}

/// Product permission checked at a resource authorization boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProductPermission {
    /// Read a result owned by the authenticated participant.
    ReadOwnResult,
    /// Mutate or command an assessment session owned by the authenticated participant.
    ManageOwnSession,
    /// Request or inspect data-rights work for the authenticated participant.
    ManageOwnDataRights,
    /// Publish or transition an instrument release within the authenticated tenant.
    PublishInstrument,
    /// Approve a research release within the authenticated tenant.
    ApproveResearchRelease,
    /// Administer tenant-scoped product configuration.
    ManageTenant,
}

/// Fail-closed product authorization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuthorizationError {
    /// A required resource or identity reference was blank or numeric-only.
    InvalidReference,
    /// The requested resource belongs to a different tenant.
    CrossTenantDenied,
    /// A participant-owned action was requested without a participant identity.
    ParticipantIdentityRequired,
    /// The authenticated participant does not own the requested resource.
    OwnerMismatch,
    /// The authenticated product roles do not grant the requested privileged operation.
    MissingRole,
}

impl Display for AuthorizationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "authorization references must be opaque non-numeric values"
            }
            Self::CrossTenantDenied => {
                "resource tenant does not match the authenticated tenant"
            }
            Self::ParticipantIdentityRequired => {
                "participant identity is required for participant-owned authorization"
            }
            Self::OwnerMismatch => {
                "resource owner does not match the authenticated participant"
            }
            Self::MissingRole => "authenticated product roles do not permit this operation",
        })
    }
}

impl Error for AuthorizationError {}

/// Server-derived authenticated product context.
///
/// The subject reference originates from an authenticated identity boundary, while
/// product roles and the optional operational participant reference are interpreted
/// only by Psychometrics Commons. Keyverse administrative roles are not implicitly
/// mapped to these roles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationContext {
    tenant_ref: String,
    subject_ref: String,
    participant_ref: Option<String>,
    roles: Vec<ProductRole>,
}

impl AuthorizationContext {
    /// Create a normalized authenticated product context.
    ///
    /// Duplicate product roles are collapsed without changing role semantics.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorizationError::InvalidReference`] when tenant, subject, or
    /// participant identity is blank or numeric-only.
    pub fn new(
        tenant_ref: &str,
        subject_ref: &str,
        participant_ref: Option<&str>,
        roles: &[ProductRole],
    ) -> Result<Self, AuthorizationError> {
        let tenant_ref = required_reference(tenant_ref)?;
        let subject_ref = required_reference(subject_ref)?;
        let participant_ref = participant_ref
            .map(required_reference)
            .transpose()?
            .map(str::to_owned);
        let mut normalized_roles = Vec::with_capacity(roles.len());
        for role in roles {
            if !normalized_roles.contains(role) {
                normalized_roles.push(*role);
            }
        }

        Ok(Self {
            tenant_ref: tenant_ref.to_owned(),
            subject_ref: subject_ref.to_owned(),
            participant_ref,
            roles: normalized_roles,
        })
    }

    /// Return the authenticated tenant reference.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the authenticated identity subject reference.
    #[must_use]
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }

    /// Return the linked operational participant reference, when present.
    #[must_use]
    pub fn participant_ref(&self) -> Option<&str> {
        self.participant_ref.as_deref()
    }

    /// Return normalized product-domain roles.
    #[must_use]
    pub fn roles(&self) -> &[ProductRole] {
        &self.roles
    }

    /// Return whether this context contains the requested product-domain role.
    #[must_use]
    pub fn has_role(&self, role: ProductRole) -> bool {
        self.roles.contains(&role)
    }
}

/// Tenant-scoped target resource used by the authorization decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceScope {
    tenant_ref: String,
    resource_ref: String,
    owner_participant_ref: Option<String>,
}

impl ResourceScope {
    /// Create a tenant-scoped resource without participant ownership semantics.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorizationError::InvalidReference`] for an invalid tenant or
    /// resource reference.
    pub fn tenant_scoped(
        tenant_ref: &str,
        resource_ref: &str,
    ) -> Result<Self, AuthorizationError> {
        Ok(Self {
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            resource_ref: required_reference(resource_ref)?.to_owned(),
            owner_participant_ref: None,
        })
    }

    /// Create a participant-owned resource inside one tenant.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorizationError::InvalidReference`] for an invalid tenant,
    /// participant, or resource reference.
    pub fn participant_owned(
        tenant_ref: &str,
        owner_participant_ref: &str,
        resource_ref: &str,
    ) -> Result<Self, AuthorizationError> {
        Ok(Self {
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            resource_ref: required_reference(resource_ref)?.to_owned(),
            owner_participant_ref: Some(required_reference(owner_participant_ref)?.to_owned()),
        })
    }

    /// Return the resource tenant reference.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the opaque resource reference.
    #[must_use]
    pub fn resource_ref(&self) -> &str {
        &self.resource_ref
    }

    /// Return the participant owner when the resource is participant-owned.
    #[must_use]
    pub fn owner_participant_ref(&self) -> Option<&str> {
        self.owner_participant_ref.as_deref()
    }
}

/// Authorize one product operation against an authenticated resource context.
///
/// Tenant mismatch is rejected before role or ownership checks, preventing a
/// privileged role or participant identity from being reused across tenants.
/// Participant-owned permissions require an exact operational participant match.
/// Instrument publication, research release approval, and tenant administration use
/// distinct product roles to preserve separation of duties.
///
/// # Errors
///
/// Returns [`AuthorizationError`] when tenant, ownership, or product-role checks
/// fail.
pub fn authorize(
    actor: &AuthorizationContext,
    resource: &ResourceScope,
    permission: ProductPermission,
) -> Result<(), AuthorizationError> {
    if actor.tenant_ref != resource.tenant_ref {
        return Err(AuthorizationError::CrossTenantDenied);
    }

    match permission {
        ProductPermission::ReadOwnResult
        | ProductPermission::ManageOwnSession
        | ProductPermission::ManageOwnDataRights => {
            let participant_ref = actor
                .participant_ref
                .as_deref()
                .ok_or(AuthorizationError::ParticipantIdentityRequired)?;
            if resource.owner_participant_ref.as_deref() != Some(participant_ref) {
                return Err(AuthorizationError::OwnerMismatch);
            }
            Ok(())
        }
        ProductPermission::PublishInstrument => require_role(actor, ProductRole::InstrumentPublisher),
        ProductPermission::ApproveResearchRelease => {
            require_role(actor, ProductRole::ResearchSteward)
        }
        ProductPermission::ManageTenant => require_role(actor, ProductRole::TenantAdministrator),
    }
}

fn require_role(
    actor: &AuthorizationContext,
    role: ProductRole,
) -> Result<(), AuthorizationError> {
    if actor.has_role(role) {
        Ok(())
    } else {
        Err(AuthorizationError::MissingRole)
    }
}

fn required_reference(reference: &str) -> Result<&str, AuthorizationError> {
    normalized_reference(reference).ok_or(AuthorizationError::InvalidReference)
}
