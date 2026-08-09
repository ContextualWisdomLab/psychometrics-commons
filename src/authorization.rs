//! Product-owned tenant and resource authorization primitives.
//!
//! Identity proof and federation remain owned by Keyverse. Psychometrics Commons
//! consumes authenticated identity claims and makes its own domain authorization
//! decisions. Tenant context is server-derived, cross-tenant access fails closed,
//! and research approval remains separated from instrument publication and tenant
//! administration. Permission checks are also bound to explicit resource kinds so
//! a valid role or participant identity cannot be reused against a different domain
//! resource through an incorrectly constructed generic scope.

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

/// Product-owned resource category at an authorization boundary.
///
/// Resource kind is part of the authorization input, not merely descriptive
/// metadata. This prevents a permission intended for one domain object from being
/// reused against another object that happens to share the same tenant or owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceKind {
    /// Immutable or superseding participant result resource.
    Result,
    /// Participant-owned assessment session resource.
    AssessmentSession,
    /// Participant-owned export/deletion request resource.
    DataRightsRequest,
    /// Tenant-scoped immutable instrument release resource.
    InstrumentRelease,
    /// Tenant-scoped research release approval resource.
    ResearchRelease,
    /// Tenant-scoped product configuration resource.
    TenantConfiguration,
}

impl ResourceKind {
    const fn requires_participant_owner(self) -> bool {
        matches!(
            self,
            Self::Result | Self::AssessmentSession | Self::DataRightsRequest
        )
    }
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

impl ProductPermission {
    const fn resource_kind(self) -> ResourceKind {
        match self {
            Self::ReadOwnResult => ResourceKind::Result,
            Self::ManageOwnSession => ResourceKind::AssessmentSession,
            Self::ManageOwnDataRights => ResourceKind::DataRightsRequest,
            Self::PublishInstrument => ResourceKind::InstrumentRelease,
            Self::ApproveResearchRelease => ResourceKind::ResearchRelease,
            Self::ManageTenant => ResourceKind::TenantConfiguration,
        }
    }
}

/// Fail-closed product authorization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuthorizationError {
    /// A required resource or identity reference was blank or numeric-only.
    InvalidReference,
    /// The requested resource belongs to a different tenant.
    CrossTenantDenied,
    /// The resource kind was constructed with an invalid ownership shape.
    ResourceOwnershipMismatch,
    /// The requested permission does not apply to the supplied resource kind.
    ResourceKindMismatch,
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
            Self::InvalidReference => "authorization references must be opaque non-numeric values",
            Self::CrossTenantDenied => "resource tenant does not match the authenticated tenant",
            Self::ResourceOwnershipMismatch => {
                "resource kind is not valid for this ownership scope"
            }
            Self::ResourceKindMismatch => "permission is not valid for this resource kind",
            Self::ParticipantIdentityRequired => {
                "participant identity is required for participant-owned authorization"
            }
            Self::OwnerMismatch => "resource owner does not match the authenticated participant",
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
    kind: ResourceKind,
    tenant_ref: String,
    resource_ref: String,
    owner_participant_ref: Option<String>,
}

impl ResourceScope {
    /// Create a tenant-scoped resource without participant ownership semantics.
    ///
    /// Only tenant-owned resource kinds are accepted. Participant-owned result,
    /// session, and data-rights resources must be constructed with
    /// [`Self::participant_owned`] so ownership cannot be omitted accidentally.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorizationError::InvalidReference`] for an invalid tenant or
    /// resource reference and [`AuthorizationError::ResourceOwnershipMismatch`]
    /// when `kind` requires an explicit participant owner.
    pub fn tenant_scoped(
        kind: ResourceKind,
        tenant_ref: &str,
        resource_ref: &str,
    ) -> Result<Self, AuthorizationError> {
        let tenant_ref = required_reference(tenant_ref)?;
        let resource_ref = required_reference(resource_ref)?;
        if kind.requires_participant_owner() {
            return Err(AuthorizationError::ResourceOwnershipMismatch);
        }
        Ok(Self {
            kind,
            tenant_ref: tenant_ref.to_owned(),
            resource_ref: resource_ref.to_owned(),
            owner_participant_ref: None,
        })
    }

    /// Create a participant-owned resource inside one tenant.
    ///
    /// Only participant-owned result, assessment-session, and data-rights kinds are
    /// accepted. Tenant-scoped instrument, research-release, and configuration
    /// resources must use [`Self::tenant_scoped`].
    ///
    /// # Errors
    ///
    /// Returns [`AuthorizationError::InvalidReference`] for an invalid tenant,
    /// participant, or resource reference and
    /// [`AuthorizationError::ResourceOwnershipMismatch`] when `kind` is tenant-only.
    pub fn participant_owned(
        kind: ResourceKind,
        tenant_ref: &str,
        owner_participant_ref: &str,
        resource_ref: &str,
    ) -> Result<Self, AuthorizationError> {
        let tenant_ref = required_reference(tenant_ref)?;
        let owner_participant_ref = required_reference(owner_participant_ref)?;
        let resource_ref = required_reference(resource_ref)?;
        if !kind.requires_participant_owner() {
            return Err(AuthorizationError::ResourceOwnershipMismatch);
        }
        Ok(Self {
            kind,
            tenant_ref: tenant_ref.to_owned(),
            resource_ref: resource_ref.to_owned(),
            owner_participant_ref: Some(owner_participant_ref.to_owned()),
        })
    }

    /// Return the resource kind that constrains applicable permissions.
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        self.kind
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
/// Tenant mismatch is rejected before resource-kind, role, or ownership checks,
/// preventing a privileged role or participant identity from being reused across
/// tenants. The requested permission must then match the exact resource kind before
/// ownership or role evaluation, preventing confused-deputy authorization caused by
/// a generic resource reference. Participant-owned permissions require an exact
/// operational participant match. Instrument publication, research release approval,
/// and tenant administration use distinct product roles to preserve separation of
/// duties.
///
/// # Errors
///
/// Returns [`AuthorizationError`] when tenant, resource-kind, ownership, or
/// product-role checks fail.
pub fn authorize(
    actor: &AuthorizationContext,
    resource: &ResourceScope,
    permission: ProductPermission,
) -> Result<(), AuthorizationError> {
    if actor.tenant_ref != resource.tenant_ref {
        return Err(AuthorizationError::CrossTenantDenied);
    }
    if resource.kind != permission.resource_kind() {
        return Err(AuthorizationError::ResourceKindMismatch);
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
        ProductPermission::PublishInstrument => {
            require_role(actor, ProductRole::InstrumentPublisher)
        }
        ProductPermission::ApproveResearchRelease => {
            require_role(actor, ProductRole::ResearchSteward)
        }
        ProductPermission::ManageTenant => require_role(actor, ProductRole::TenantAdministrator),
    }
}

fn require_role(actor: &AuthorizationContext, role: ProductRole) -> Result<(), AuthorizationError> {
    if actor.has_role(role) {
        Ok(())
    } else {
        Err(AuthorizationError::MissingRole)
    }
}

fn required_reference(reference: &str) -> Result<&str, AuthorizationError> {
    normalized_reference(reference).ok_or(AuthorizationError::InvalidReference)
}