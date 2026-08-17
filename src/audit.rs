//! Immutable, purpose-bound product audit evidence.
//!
//! Audit records capture the minimum product metadata required to prove who attempted a
//! security- or governance-relevant action, under which tenant and purpose, against which
//! resource, and with what outcome. Raw assessment responses, credentials, reflective text, and
//! provider payloads do not belong in this contract; callers bind richer evidence by canonical
//! SHA-256 digest instead.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Outcome recorded for one auditable product action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuditOutcome {
    /// The authorized product action completed successfully.
    Succeeded,
    /// Authorization or a governing policy denied the attempted action.
    Denied,
    /// The action was authorized to start but failed before successful completion.
    Failed,
}

impl AuditOutcome {
    /// Return the stable lowercase persistence and event-contract code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }

    /// Reconstruct one outcome from its stable persistence code.
    ///
    /// # Errors
    ///
    /// Returns [`AuditEvidenceError::InvalidOutcome`] when the stored value is unknown.
    pub fn from_code(code: &str) -> Result<Self, AuditEvidenceError> {
        match code {
            "succeeded" => Ok(Self::Succeeded),
            "denied" => Ok(Self::Denied),
            "failed" => Ok(Self::Failed),
            _ => Err(AuditEvidenceError::InvalidOutcome),
        }
    }
}

/// Borrowed inputs required to create one immutable audit record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditEvidenceInput<'a> {
    /// Globally opaque identity of this audit event.
    pub audit_event_ref: &'a str,
    /// Product tenant in whose authorization context the action occurred.
    pub tenant_ref: &'a str,
    /// Opaque actor or anonymous-session subject responsible for the attempt.
    pub actor_ref: &'a str,
    /// Stable lowercase purpose code governing why the action was attempted.
    pub purpose_code: &'a str,
    /// Stable lowercase action code naming the product operation.
    pub action_code: &'a str,
    /// Opaque product resource against which the action was attempted.
    pub resource_ref: &'a str,
    /// Server-observed outcome of the attempted action.
    pub outcome: AuditOutcome,
    /// Canonical digest binding any separately retained supporting evidence.
    pub evidence_digest: &'a str,
    /// Server-authoritative event time as Unix milliseconds.
    pub occurred_at_unix_ms: u64,
}

/// Immutable product audit evidence safe for durable operational persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvidence {
    audit_event_ref: String,
    tenant_ref: String,
    actor_ref: String,
    purpose_code: String,
    action_code: String,
    resource_ref: String,
    outcome: AuditOutcome,
    evidence_digest: String,
    occurred_at_unix_ms: u64,
}

impl AuditEvidence {
    /// Validate and freeze one purpose-bound audit record.
    ///
    /// Product references must already use their canonical exact spelling. Purpose/action codes
    /// are intentionally restricted to lowercase ASCII machine tokens so later policy and
    /// analytics layers cannot silently alias the same operation. The supporting-evidence digest
    /// must already be canonical lowercase `sha256:<64 hex>` evidence; this domain never receives
    /// or hashes sensitive raw payloads.
    ///
    /// # Errors
    ///
    /// Returns [`AuditEvidenceError::InvalidReference`] for a blank, numeric-like, or aliased
    /// product reference, [`AuditEvidenceError::InvalidCode`] for an unstable purpose/action code,
    /// [`AuditEvidenceError::InvalidDigest`] for noncanonical supporting-evidence identity, or
    /// [`AuditEvidenceError::InvalidTimestamp`] for a zero server timestamp.
    pub fn new(input: AuditEvidenceInput<'_>) -> Result<Self, AuditEvidenceError> {
        let audit_event_ref = required_reference(input.audit_event_ref)?;
        let tenant_ref = required_reference(input.tenant_ref)?;
        let actor_ref = required_reference(input.actor_ref)?;
        let resource_ref = required_reference(input.resource_ref)?;
        if !valid_machine_code(input.purpose_code) || !valid_machine_code(input.action_code) {
            return Err(AuditEvidenceError::InvalidCode);
        }
        if !canonical_sha256_digest(input.evidence_digest) {
            return Err(AuditEvidenceError::InvalidDigest);
        }
        if input.occurred_at_unix_ms == 0 {
            return Err(AuditEvidenceError::InvalidTimestamp);
        }

        Ok(Self {
            audit_event_ref: audit_event_ref.to_owned(),
            tenant_ref: tenant_ref.to_owned(),
            actor_ref: actor_ref.to_owned(),
            purpose_code: input.purpose_code.to_owned(),
            action_code: input.action_code.to_owned(),
            resource_ref: resource_ref.to_owned(),
            outcome: input.outcome,
            evidence_digest: input.evidence_digest.to_owned(),
            occurred_at_unix_ms: input.occurred_at_unix_ms,
        })
    }

    /// Return the globally opaque audit-event reference.
    #[must_use]
    pub fn audit_event_ref(&self) -> &str {
        &self.audit_event_ref
    }

    /// Return the exact tenant authorization context.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the opaque actor or anonymous-session subject reference.
    #[must_use]
    pub fn actor_ref(&self) -> &str {
        &self.actor_ref
    }

    /// Return the stable purpose code governing this action.
    #[must_use]
    pub fn purpose_code(&self) -> &str {
        &self.purpose_code
    }

    /// Return the stable product action code.
    #[must_use]
    pub fn action_code(&self) -> &str {
        &self.action_code
    }

    /// Return the exact product resource reference.
    #[must_use]
    pub fn resource_ref(&self) -> &str {
        &self.resource_ref
    }

    /// Return the server-observed action outcome.
    #[must_use]
    pub const fn outcome(&self) -> AuditOutcome {
        self.outcome
    }

    /// Return the canonical digest of separately retained supporting evidence.
    #[must_use]
    pub fn evidence_digest(&self) -> &str {
        &self.evidence_digest
    }

    /// Return the server-authoritative event time as Unix milliseconds.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }
}

/// Fail-closed audit-evidence construction or reload error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuditEvidenceError {
    /// A product identity was blank, numeric-like, or not exactly canonical.
    InvalidReference,
    /// A purpose or action code was not a stable lowercase ASCII machine token.
    InvalidCode,
    /// A supporting-evidence digest was not canonical lowercase SHA-256 evidence.
    InvalidDigest,
    /// A server event timestamp was zero.
    InvalidTimestamp,
    /// A persisted action outcome was not one of the supported stable values.
    InvalidOutcome,
}

impl Display for AuditEvidenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "audit evidence references must be exact canonical opaque non-numeric values"
            }
            Self::InvalidCode => {
                "audit purpose and action codes must be lowercase ASCII machine tokens"
            }
            Self::InvalidDigest => {
                "audit supporting evidence digest must be canonical lowercase SHA-256 evidence"
            }
            Self::InvalidTimestamp => "audit event timestamp must be positive",
            Self::InvalidOutcome => "audit outcome code is unsupported",
        })
    }
}

impl Error for AuditEvidenceError {}

fn required_reference(reference: &str) -> Result<&str, AuditEvidenceError> {
    match normalized_reference(reference) {
        Some(normalized) if normalized == reference => Ok(reference),
        _ => Err(AuditEvidenceError::InvalidReference),
    }
}

fn valid_machine_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn canonical_sha256_digest(digest: &str) -> bool {
    let Some(hexadecimal) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hexadecimal.len() == 64
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::{AuditEvidenceError, AuditOutcome};

    #[test]
    fn persisted_outcome_codes_round_trip_and_unknown_values_fail_closed() {
        for outcome in [
            AuditOutcome::Succeeded,
            AuditOutcome::Denied,
            AuditOutcome::Failed,
        ] {
            assert_eq!(AuditOutcome::from_code(outcome.as_code()).unwrap(), outcome);
        }
        assert_eq!(
            AuditOutcome::from_code("unknown").unwrap_err(),
            AuditEvidenceError::InvalidOutcome
        );
    }
}
