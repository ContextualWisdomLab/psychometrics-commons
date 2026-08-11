//! Operation-scoped runtime readiness and capability health semantics.
//!
//! Liveness, readiness, dependency capability health, backlog health, and data
//! integrity are deliberately separate signals. An optional dependency outage must
//! not make unrelated work unavailable, while unknown integrity or a stalled durable
//! backlog fails closed for new state-changing work.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Health of durable work waiting for processing or delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BacklogHealth {
    /// Durable work remains within the deployment's measured operating bounds.
    WithinBounds,
    /// Durable work is known to be stalled and must not accept more state-changing work.
    Stalled,
    /// Backlog health cannot currently be established.
    Unknown,
}

impl BacklogHealth {
    const fn accepts_new_work(self) -> bool {
        matches!(self, Self::WithinBounds)
    }
}

/// Integrity state of the active schema, migrations, digests, and reconciliation evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataIntegrityHealth {
    /// Required integrity evidence is compatible with the running application.
    Verified,
    /// A schema, migration, digest, or reconciliation incompatibility is known.
    Incompatible,
    /// Integrity compatibility cannot currently be established.
    Unknown,
}

impl DataIntegrityHealth {
    const fn accepts_new_work(self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// Observable state of one independently degradable product capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CapabilityState {
    /// The capability is operating normally.
    Available,
    /// The capability is impaired but may still safely accept bounded new work.
    Degraded,
    /// The capability is unavailable.
    Unavailable,
    /// The capability state cannot currently be established.
    Unknown,
}

/// Fail-closed health-contract error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HealthContractError {
    /// A capability reference was blank or numeric-only instead of opaque.
    InvalidReference,
    /// One snapshot repeated the same capability reference.
    DuplicateCapabilityReference,
    /// An unavailable/unknown capability was incorrectly marked safe for new work.
    InconsistentCapabilityReadiness,
}

impl Display for HealthContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "health capability references must be opaque non-numeric values"
            }
            Self::DuplicateCapabilityReference => {
                "health capability references must be unique within one snapshot"
            }
            Self::InconsistentCapabilityReadiness => {
                "unavailable or unknown capability cannot accept new work"
            }
        })
    }
}

impl Error for HealthContractError {}

/// Health evidence for one product capability or dependency boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityHealth {
    capability_ref: String,
    state: CapabilityState,
    accepts_new_work: bool,
}

impl CapabilityHealth {
    /// Create one capability-health observation.
    ///
    /// `accepts_new_work` is kept separate from the descriptive state so an impaired
    /// capability can explicitly remain usable for a bounded operation without
    /// treating every `Degraded` state as globally ready. An `Unavailable` or
    /// `Unknown` capability can never claim readiness.
    ///
    /// # Errors
    ///
    /// Returns [`HealthContractError::InvalidReference`] when `capability_ref` is not
    /// an opaque product reference, or
    /// [`HealthContractError::InconsistentCapabilityReadiness`] for contradictory
    /// unavailable/unknown readiness evidence.
    pub fn new(
        capability_ref: &str,
        state: CapabilityState,
        accepts_new_work: bool,
    ) -> Result<Self, HealthContractError> {
        let capability_ref =
            normalized_reference(capability_ref).ok_or(HealthContractError::InvalidReference)?;
        if accepts_new_work
            && matches!(
                state,
                CapabilityState::Unavailable | CapabilityState::Unknown
            )
        {
            return Err(HealthContractError::InconsistentCapabilityReadiness);
        }
        Ok(Self {
            capability_ref: capability_ref.to_owned(),
            state,
            accepts_new_work,
        })
    }

    /// Return the opaque capability reference.
    #[must_use]
    pub fn capability_ref(&self) -> &str {
        &self.capability_ref
    }

    /// Return the observed capability state.
    #[must_use]
    pub const fn state(&self) -> CapabilityState {
        self.state
    }

    /// Return whether this capability may safely accept bounded new work.
    #[must_use]
    pub const fn accepts_new_work(&self) -> bool {
        self.accepts_new_work
    }
}

/// Point-in-time runtime health used to answer operation-specific readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeHealthSnapshot {
    live: bool,
    backlog_health: BacklogHealth,
    data_integrity_health: DataIntegrityHealth,
    capabilities: Vec<CapabilityHealth>,
}

impl RuntimeHealthSnapshot {
    /// Create a health snapshot with unique capability identities.
    ///
    /// # Errors
    ///
    /// Returns [`HealthContractError::DuplicateCapabilityReference`] if the snapshot
    /// contains two observations for the same capability identity.
    pub fn new(
        live: bool,
        backlog_health: BacklogHealth,
        data_integrity_health: DataIntegrityHealth,
        capabilities: Vec<CapabilityHealth>,
    ) -> Result<Self, HealthContractError> {
        for (index, capability) in capabilities.iter().enumerate() {
            if capabilities[..index]
                .iter()
                .any(|existing| existing.capability_ref == capability.capability_ref)
            {
                return Err(HealthContractError::DuplicateCapabilityReference);
            }
        }
        Ok(Self {
            live,
            backlog_health,
            data_integrity_health,
            capabilities,
        })
    }

    /// Return whether the process itself can make progress.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.live
    }

    /// Return current durable-work backlog health.
    #[must_use]
    pub const fn backlog_health(&self) -> BacklogHealth {
        self.backlog_health
    }

    /// Return current schema/migration/digest/reconciliation integrity health.
    #[must_use]
    pub const fn data_integrity_health(&self) -> DataIntegrityHealth {
        self.data_integrity_health
    }

    /// Return all independently observable capability observations.
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityHealth] {
        &self.capabilities
    }

    /// Find one capability observation by exact opaque reference.
    #[must_use]
    pub fn capability(&self, capability_ref: &str) -> Option<&CapabilityHealth> {
        self.capabilities
            .iter()
            .find(|capability| capability.capability_ref == capability_ref)
    }

    /// Return whether new state-changing work requiring `required_capabilities` is safe.
    ///
    /// Readiness fails closed when the process is not live, backlog or integrity
    /// evidence is unsafe/unknown, a required capability is unknown, or a required
    /// capability cannot accept new work. Optional capabilities do not participate
    /// unless the caller names them for the operation being considered.
    #[must_use]
    pub fn is_ready_for(&self, required_capabilities: &[&str]) -> bool {
        self.live
            && self.backlog_health.accepts_new_work()
            && self.data_integrity_health.accepts_new_work()
            && required_capabilities.iter().all(|required| {
                self.capability(required)
                    .is_some_and(CapabilityHealth::accepts_new_work)
            })
    }
}
