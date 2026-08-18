//! Persistable live measurement-session provenance without scoring kernels.
//!
//! Psychometrics Commons owns session membership, purpose-specific consent,
//! append-only audit evidence, and the export-snapshot pointer. Numeric scores,
//! IRT, linking, and identity-link history stay outside this aggregate.

use crate::authorization::{
    authorize, AuthorizationContext, AuthorizationError, ProductPermission, ResourceKind,
    ResourceScope,
};
use crate::consent::{ConsentDecision, ConsentPurpose};
use crate::instrument::valid_sha256_digest;
use crate::reference::normalized_reference;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Purpose bound to the measurement-session persist/reload encryption key.
pub const MEASUREMENT_SESSION_PERSIST_PURPOSE: &str = "measurement_session_persist";

/// Fail-closed error for live measurement-session construction or sealing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MeasurementSessionError {
    /// A session, tenant, participant, event, actor, or snapshot reference was invalid.
    InvalidReference,
    /// A lifecycle or enrollment timestamp was zero.
    InvalidTimestamp,
    /// The export-snapshot digest was not a canonical SHA-256 digest.
    InvalidContentDigest,
    /// The owner is missing from session membership.
    OwnerNotMember,
    /// The same participant was enrolled more than once.
    DuplicateMembership,
    /// A consent or audit event reference was reused.
    DuplicateEventIdentity,
    /// A consent record names a participant who is not a session member.
    ConsentParticipantNotMember,
    /// The encryption key purpose is not measurement-session persist.
    InvalidEncryptionPurpose,
    /// Authenticated encryption rejected the payload or key.
    SealingFailed,
}

impl Display for MeasurementSessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "measurement session references must be opaque non-numeric values"
            }
            Self::InvalidTimestamp => "measurement session timestamps must be greater than zero",
            Self::InvalidContentDigest => {
                "export snapshot digest must be sha256: and 64 lowercase hex digits"
            }
            Self::OwnerNotMember => "session owner must also be recorded in session membership",
            Self::DuplicateMembership => "session membership participant references must be unique",
            Self::DuplicateEventIdentity => {
                "consent and audit event references must be unique in one session"
            }
            Self::ConsentParticipantNotMember => {
                "consent records may name only enrolled session members"
            }
            Self::InvalidEncryptionPurpose => {
                "session encryption keys must use the measurement_session_persist purpose"
            }
            Self::SealingFailed => "measurement session payload could not be sealed or opened",
        })
    }
}

impl Error for MeasurementSessionError {}

/// Purpose-bound AES-256-GCM key used only for measurement-session persist/reload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionEncryptionKey {
    purpose_ref: String,
    key_bytes: [u8; 32],
}

impl SessionEncryptionKey {
    /// Bind a 32-byte key to the measurement-session persist purpose.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::InvalidReference`] when `purpose_ref`
    /// is blank or numeric-like, and
    /// [`MeasurementSessionError::InvalidEncryptionPurpose`] when the purpose is
    /// not [`MEASUREMENT_SESSION_PERSIST_PURPOSE`].
    pub fn new(purpose_ref: &str, key_bytes: [u8; 32]) -> Result<Self, MeasurementSessionError> {
        let purpose_ref = required_reference(purpose_ref)?;
        if purpose_ref != MEASUREMENT_SESSION_PERSIST_PURPOSE {
            return Err(MeasurementSessionError::InvalidEncryptionPurpose);
        }
        Ok(Self {
            purpose_ref: purpose_ref.to_owned(),
            key_bytes,
        })
    }

    /// Return the purpose that must be used as encryption additional data.
    #[must_use]
    pub fn purpose_ref(&self) -> &str {
        &self.purpose_ref
    }

    /// Seal one UTF-8 payload with purpose-bound additional authenticated data.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::SealingFailed`] when AES-256-GCM
    /// rejects the key or payload.
    pub fn seal(
        &self,
        nonce_material: &str,
        associated_data: &str,
        plaintext: &str,
    ) -> Result<SealedPayload, MeasurementSessionError> {
        Ok(self.seal_bytes(nonce_material, associated_data, plaintext.as_bytes()))
    }

    /// Seal raw bytes so tests can prove non-UTF-8 plaintext fails closed on open.
    pub(crate) fn seal_bytes(
        &self,
        nonce_material: &str,
        associated_data: &str,
        plaintext: &[u8],
    ) -> SealedPayload {
        let nonce = nonce_for(nonce_material);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key_bytes));
        // AES-256-GCM encryption is infallible for a 12-byte nonce; keep the
        // Result mapper only for authenticated decrypt, which can fail closed.
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: associated_data.as_bytes(),
                },
            )
            .unwrap_or_else(empty_ciphertext_on_infallible_encrypt);
        SealedPayload { nonce, ciphertext }
    }

    /// Open one sealed payload and fail closed on key, nonce, or AAD mismatch.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::SealingFailed`] when authentication
    /// fails.
    pub fn open(
        &self,
        sealed: &SealedPayload,
        associated_data: &str,
    ) -> Result<String, MeasurementSessionError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key_bytes));
        let plaintext = map_aead(cipher.decrypt(
            Nonce::from_slice(&sealed.nonce),
            Payload {
                msg: &sealed.ciphertext,
                aad: associated_data.as_bytes(),
            },
        ))?;
        String::from_utf8(plaintext).map_err(|_| MeasurementSessionError::SealingFailed)
    }
}

/// AES-256-GCM nonce plus ciphertext for one stored session field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedPayload {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

impl SealedPayload {
    /// Reconstruct a stored nonce and ciphertext after a durable load.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::SealingFailed`] when the nonce is not
    /// 12 bytes or the ciphertext is empty.
    pub fn from_stored(nonce: &[u8], ciphertext: Vec<u8>) -> Result<Self, MeasurementSessionError> {
        let nonce: [u8; 12] = nonce
            .try_into()
            .map_err(|_| MeasurementSessionError::SealingFailed)?;
        if ciphertext.is_empty() {
            return Err(MeasurementSessionError::SealingFailed);
        }
        Ok(Self { nonce, ciphertext })
    }

    /// Return the 12-byte AES-GCM nonce.
    #[must_use]
    pub fn nonce(&self) -> &[u8; 12] {
        &self.nonce
    }

    /// Return the authenticated ciphertext bytes.
    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

/// One enrolled participant in a live measurement session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionMembership {
    participant_ref: String,
    tenant_ref: String,
    created_at_unix_ms: u64,
    enrolled_at_unix_ms: u64,
}

impl SessionMembership {
    /// Record one participant's membership without copying identity-link history.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::InvalidReference`] or
    /// [`MeasurementSessionError::InvalidTimestamp`].
    pub fn new(
        participant_ref: &str,
        tenant_ref: &str,
        created_at_unix_ms: u64,
        enrolled_at_unix_ms: u64,
    ) -> Result<Self, MeasurementSessionError> {
        if created_at_unix_ms == 0 || enrolled_at_unix_ms == 0 {
            return Err(MeasurementSessionError::InvalidTimestamp);
        }
        Ok(Self {
            participant_ref: required_reference(participant_ref)?.to_owned(),
            tenant_ref: required_reference(tenant_ref)?.to_owned(),
            created_at_unix_ms,
            enrolled_at_unix_ms,
        })
    }

    /// Return the stable operational participant reference.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the tenant that owns this participant record.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the server-authoritative participant creation time.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return the server-authoritative enrollment time in this session.
    #[must_use]
    pub const fn enrolled_at_unix_ms(&self) -> u64 {
        self.enrolled_at_unix_ms
    }
}

/// One purpose-specific consent decision stored with a live session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionConsentRecord {
    event_ref: String,
    participant_ref: String,
    purpose: ConsentPurpose,
    decision: ConsentDecision,
    consent_form_version_ref: String,
    research_scope_ref: Option<String>,
    decided_at_unix_ms: u64,
}

impl SessionConsentRecord {
    /// Capture one grant or revoke without masking construct-relevant fields.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::InvalidReference`] or
    /// [`MeasurementSessionError::InvalidTimestamp`].
    pub fn new(
        event_ref: &str,
        participant_ref: &str,
        purpose: ConsentPurpose,
        decision: ConsentDecision,
        consent_form_version_ref: &str,
        research_scope_ref: Option<&str>,
        decided_at_unix_ms: u64,
    ) -> Result<Self, MeasurementSessionError> {
        if decided_at_unix_ms == 0 {
            return Err(MeasurementSessionError::InvalidTimestamp);
        }
        let research_scope_ref = research_scope_ref
            .map(required_reference)
            .transpose()?
            .map(str::to_owned);
        Ok(Self {
            event_ref: required_reference(event_ref)?.to_owned(),
            participant_ref: required_reference(participant_ref)?.to_owned(),
            purpose,
            decision,
            consent_form_version_ref: required_reference(consent_form_version_ref)?.to_owned(),
            research_scope_ref,
            decided_at_unix_ms,
        })
    }

    /// Return the opaque consent-event reference.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the enrolled participant who decided.
    #[must_use]
    pub fn participant_ref(&self) -> &str {
        &self.participant_ref
    }

    /// Return the independently revocable consent purpose.
    #[must_use]
    pub const fn purpose(&self) -> ConsentPurpose {
        self.purpose
    }

    /// Return whether the purpose was granted or revoked.
    #[must_use]
    pub const fn decision(&self) -> ConsentDecision {
        self.decision
    }

    /// Return the exact consent-form version shown for this decision.
    #[must_use]
    pub fn consent_form_version_ref(&self) -> &str {
        &self.consent_form_version_ref
    }

    /// Return the research scope when this is a research-purpose decision.
    #[must_use]
    pub fn research_scope_ref(&self) -> Option<&str> {
        self.research_scope_ref.as_deref()
    }

    /// Return the server-authoritative decision time.
    #[must_use]
    pub const fn decided_at_unix_ms(&self) -> u64 {
        self.decided_at_unix_ms
    }

    pub(crate) fn sealed_payload(
        &self,
        key: &SessionEncryptionKey,
        session_ref: &str,
    ) -> SealedPayload {
        key.seal_bytes(
            &format!("{session_ref}\0consent\0{}", self.event_ref),
            &associated_data(session_ref, "consent_record", &self.event_ref),
            self.canonical_payload().as_bytes(),
        )
    }

    pub(crate) fn from_sealed(
        event_ref: &str,
        participant_ref: &str,
        key: &SessionEncryptionKey,
        session_ref: &str,
        sealed: &SealedPayload,
    ) -> Result<Self, MeasurementSessionError> {
        let payload = key.open(
            sealed,
            &associated_data(session_ref, "consent_record", event_ref),
        )?;
        let mut fields = payload.split('\u{1f}');
        let purpose = parse_purpose(fields.next().unwrap_or_default())?;
        let decision = parse_decision(fields.next().unwrap_or_default())?;
        let consent_form_version_ref = fields.next().unwrap_or_default();
        let research_scope = fields.next().unwrap_or_default();
        let decided_at = fields
            .next()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| MeasurementSessionError::SealingFailed)?;
        let research_scope_ref = if research_scope.is_empty() {
            None
        } else {
            Some(research_scope)
        };
        Self::new(
            event_ref,
            participant_ref,
            purpose,
            decision,
            consent_form_version_ref,
            research_scope_ref,
            decided_at,
        )
    }

    fn canonical_payload(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            purpose_name(self.purpose),
            decision_name(self.decision),
            self.consent_form_version_ref,
            self.research_scope_ref.as_deref().unwrap_or(""),
            self.decided_at_unix_ms
        )
    }
}

/// One append-only audit event retained with a live session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionAuditEvent {
    event_ref: String,
    actor_ref: String,
    action_name: String,
    purpose_ref: String,
    evidence_digest: String,
    occurred_at_unix_ms: u64,
}

impl SessionAuditEvent {
    /// Record one purpose-bound audit event without masking operational fields.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::InvalidReference`],
    /// [`MeasurementSessionError::InvalidTimestamp`], or
    /// [`MeasurementSessionError::InvalidContentDigest`].
    pub fn new(
        event_ref: &str,
        actor_ref: &str,
        action_name: &str,
        purpose_ref: &str,
        evidence_digest: &str,
        occurred_at_unix_ms: u64,
    ) -> Result<Self, MeasurementSessionError> {
        if occurred_at_unix_ms == 0 {
            return Err(MeasurementSessionError::InvalidTimestamp);
        }
        if !valid_sha256_digest(evidence_digest) {
            return Err(MeasurementSessionError::InvalidContentDigest);
        }
        Ok(Self {
            event_ref: required_reference(event_ref)?.to_owned(),
            actor_ref: required_reference(actor_ref)?.to_owned(),
            action_name: required_reference(action_name)?.to_owned(),
            purpose_ref: required_reference(purpose_ref)?.to_owned(),
            evidence_digest: evidence_digest.to_owned(),
            occurred_at_unix_ms,
        })
    }

    /// Return the opaque audit-event reference.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the actor who produced this audit event.
    #[must_use]
    pub fn actor_ref(&self) -> &str {
        &self.actor_ref
    }

    /// Return the two-or-more-word action recorded by the event.
    #[must_use]
    pub fn action_name(&self) -> &str {
        &self.action_name
    }

    /// Return the purpose that limited this audit record.
    #[must_use]
    pub fn purpose_ref(&self) -> &str {
        &self.purpose_ref
    }

    /// Return the SHA-256 digest of the audit evidence payload.
    #[must_use]
    pub fn evidence_digest(&self) -> &str {
        &self.evidence_digest
    }

    /// Return the server-authoritative audit time.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }

    pub(crate) fn sealed_payload(
        &self,
        key: &SessionEncryptionKey,
        session_ref: &str,
    ) -> SealedPayload {
        key.seal_bytes(
            &format!("{session_ref}\0audit\0{}", self.event_ref),
            &associated_data(session_ref, "audit_event", &self.event_ref),
            self.canonical_payload().as_bytes(),
        )
    }

    pub(crate) fn from_sealed(
        event_ref: &str,
        actor_ref: &str,
        occurred_at_unix_ms: u64,
        key: &SessionEncryptionKey,
        session_ref: &str,
        sealed: &SealedPayload,
    ) -> Result<Self, MeasurementSessionError> {
        let payload = key.open(
            sealed,
            &associated_data(session_ref, "audit_event", event_ref),
        )?;
        let mut fields = payload.split('\u{1f}');
        let action_name = fields.next().unwrap_or_default();
        let purpose_ref = fields.next().unwrap_or_default();
        let evidence_digest = fields.next().unwrap_or_default();
        Self::new(
            event_ref,
            actor_ref,
            action_name,
            purpose_ref,
            evidence_digest,
            occurred_at_unix_ms,
        )
    }

    fn canonical_payload(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}",
            self.action_name, self.purpose_ref, self.evidence_digest
        )
    }
}

/// Content-addressed pointer to an export snapshot. This is not a score.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportSnapshotPointer {
    snapshot_ref: String,
    request_ref: String,
    content_digest: String,
    created_at_unix_ms: u64,
}

impl ExportSnapshotPointer {
    /// Point at one immutable export snapshot by reference and digest.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError::InvalidReference`],
    /// [`MeasurementSessionError::InvalidTimestamp`], or
    /// [`MeasurementSessionError::InvalidContentDigest`].
    pub fn new(
        snapshot_ref: &str,
        request_ref: &str,
        content_digest: &str,
        created_at_unix_ms: u64,
    ) -> Result<Self, MeasurementSessionError> {
        if created_at_unix_ms == 0 {
            return Err(MeasurementSessionError::InvalidTimestamp);
        }
        if !valid_sha256_digest(content_digest) {
            return Err(MeasurementSessionError::InvalidContentDigest);
        }
        Ok(Self {
            snapshot_ref: required_reference(snapshot_ref)?.to_owned(),
            request_ref: required_reference(request_ref)?.to_owned(),
            content_digest: content_digest.to_owned(),
            created_at_unix_ms,
        })
    }

    /// Return the opaque export-snapshot reference.
    #[must_use]
    pub fn snapshot_ref(&self) -> &str {
        &self.snapshot_ref
    }

    /// Return the data-rights request that produced the snapshot.
    #[must_use]
    pub fn request_ref(&self) -> &str {
        &self.request_ref
    }

    /// Return the SHA-256 digest of the export artifact, never a numeric score.
    #[must_use]
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    /// Return the server-authoritative pointer creation time.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
}

/// Borrowed construction input for one live measurement session.
pub struct MeasurementSessionInput {
    /// Opaque session reference.
    pub session_ref: String,
    /// Tenant that owns the session.
    pub tenant_ref: String,
    /// Participant used for `ManageOwnSession` authorization.
    pub owner_participant_ref: String,
    /// Server-authoritative session creation time.
    pub created_at_unix_ms: u64,
    /// Enrolled participants, including the owner.
    pub memberships: Vec<SessionMembership>,
    /// Purpose-specific consent records for enrolled members.
    pub consent_records: Vec<SessionConsentRecord>,
    /// Append-only audit events.
    pub audit_events: Vec<SessionAuditEvent>,
    /// Optional export-snapshot pointer. This is not a score.
    pub export_snapshot_pointer: Option<ExportSnapshotPointer>,
}

/// Live measurement session that commons can persist and restore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementSession {
    session_ref: String,
    tenant_ref: String,
    owner_participant_ref: String,
    created_at_unix_ms: u64,
    memberships: Vec<SessionMembership>,
    consent_records: Vec<SessionConsentRecord>,
    audit_events: Vec<SessionAuditEvent>,
    export_snapshot_pointer: Option<ExportSnapshotPointer>,
}

impl MeasurementSession {
    /// Assemble one live session from membership, consent, audit, and export pointer.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSessionError`] when references, timestamps,
    /// membership, or event uniqueness fail closed.
    pub fn new(input: MeasurementSessionInput) -> Result<Self, MeasurementSessionError> {
        if input.created_at_unix_ms == 0 {
            return Err(MeasurementSessionError::InvalidTimestamp);
        }
        let session_ref = required_reference(&input.session_ref)?.to_owned();
        let tenant_ref = required_reference(&input.tenant_ref)?.to_owned();
        let owner_participant_ref = required_reference(&input.owner_participant_ref)?.to_owned();
        let MeasurementSessionInput {
            memberships,
            consent_records,
            audit_events,
            export_snapshot_pointer,
            created_at_unix_ms,
            ..
        } = input;
        if memberships.is_empty() {
            return Err(MeasurementSessionError::OwnerNotMember);
        }
        let mut seen_members = Vec::new();
        let mut owner_found = false;
        for membership in &memberships {
            if membership.tenant_ref() != tenant_ref {
                return Err(MeasurementSessionError::InvalidReference);
            }
            if seen_members.contains(&membership.participant_ref) {
                return Err(MeasurementSessionError::DuplicateMembership);
            }
            if membership.participant_ref() == owner_participant_ref {
                owner_found = true;
            }
            seen_members.push(membership.participant_ref.clone());
        }
        if !owner_found {
            return Err(MeasurementSessionError::OwnerNotMember);
        }
        let mut memberships = memberships;
        memberships.sort_by(|left, right| left.participant_ref.cmp(&right.participant_ref));
        let mut seen_consent = Vec::new();
        for record in &consent_records {
            if seen_consent.contains(&record.event_ref) {
                return Err(MeasurementSessionError::DuplicateEventIdentity);
            }
            if !seen_members.contains(&record.participant_ref) {
                return Err(MeasurementSessionError::ConsentParticipantNotMember);
            }
            seen_consent.push(record.event_ref.clone());
        }
        let mut seen_audit = Vec::new();
        for event in &audit_events {
            if seen_audit.contains(&event.event_ref) {
                return Err(MeasurementSessionError::DuplicateEventIdentity);
            }
            seen_audit.push(event.event_ref.clone());
        }
        let mut consent_records = consent_records;
        consent_records.sort_by(|left, right| left.event_ref.cmp(&right.event_ref));
        let mut audit_events = audit_events;
        audit_events.sort_by(|left, right| left.event_ref.cmp(&right.event_ref));
        Ok(Self {
            session_ref,
            tenant_ref,
            owner_participant_ref,
            created_at_unix_ms,
            memberships,
            consent_records,
            audit_events,
            export_snapshot_pointer,
        })
    }

    /// Return the opaque session reference.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the tenant that owns this session.
    #[must_use]
    pub fn tenant_ref(&self) -> &str {
        &self.tenant_ref
    }

    /// Return the participant who owns this session for authorization.
    #[must_use]
    pub fn owner_participant_ref(&self) -> &str {
        &self.owner_participant_ref
    }

    /// Return the server-authoritative session creation time.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return enrolled participant membership in stored order.
    #[must_use]
    pub fn memberships(&self) -> &[SessionMembership] {
        &self.memberships
    }

    /// Return purpose-specific consent records in stored order.
    #[must_use]
    pub fn consent_records(&self) -> &[SessionConsentRecord] {
        &self.consent_records
    }

    /// Return append-only audit events in stored order.
    #[must_use]
    pub fn audit_events(&self) -> &[SessionAuditEvent] {
        &self.audit_events
    }

    /// Return the export-snapshot pointer when one has been attached.
    #[must_use]
    pub fn export_snapshot_pointer(&self) -> Option<&ExportSnapshotPointer> {
        self.export_snapshot_pointer.as_ref()
    }

    /// Return whether the latest service-operation consent for `participant_ref` is granted.
    #[must_use]
    pub fn service_operation_is_granted(&self, participant_ref: &str) -> bool {
        self.consent_records
            .iter()
            .rev()
            .find(|record| {
                record.participant_ref() == participant_ref
                    && record.purpose() == ConsentPurpose::ServiceOperation
            })
            .is_some_and(|record| record.decision() == ConsentDecision::Granted)
    }

    /// Return the canonical provenance bytes used for byte-for-byte reload equality.
    #[must_use]
    pub fn provenance_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_field(&mut bytes, &self.session_ref);
        push_field(&mut bytes, &self.tenant_ref);
        push_field(&mut bytes, &self.owner_participant_ref);
        push_field(&mut bytes, &self.created_at_unix_ms.to_string());
        for membership in &self.memberships {
            push_field(&mut bytes, membership.participant_ref());
            push_field(&mut bytes, membership.tenant_ref());
            push_field(&mut bytes, &membership.created_at_unix_ms().to_string());
            push_field(&mut bytes, &membership.enrolled_at_unix_ms().to_string());
        }
        for record in &self.consent_records {
            push_field(&mut bytes, &record.canonical_payload());
            push_field(&mut bytes, record.event_ref());
            push_field(&mut bytes, record.participant_ref());
        }
        for event in &self.audit_events {
            push_field(&mut bytes, &event.canonical_payload());
            push_field(&mut bytes, event.event_ref());
            push_field(&mut bytes, event.actor_ref());
            push_field(&mut bytes, &event.occurred_at_unix_ms().to_string());
        }
        match &self.export_snapshot_pointer {
            Some(pointer) => {
                push_field(&mut bytes, "pointer");
                push_field(&mut bytes, pointer.snapshot_ref());
                push_field(&mut bytes, pointer.request_ref());
                push_field(&mut bytes, pointer.content_digest());
                push_field(&mut bytes, &pointer.created_at_unix_ms().to_string());
            }
            None => push_field(&mut bytes, "none"),
        }
        bytes
    }

    /// Return the SHA-256 digest of [`Self::provenance_bytes`].
    #[must_use]
    pub fn provenance_digest(&self) -> String {
        format!("sha256:{:x}", Sha256::digest(self.provenance_bytes()))
    }
}

/// Authorize persist or reload of one live measurement session.
///
/// The only accepted permission is [`ProductPermission::ManageOwnSession`] on
/// [`ResourceKind::AssessmentSession`]. Consent or data-rights permissions cannot
/// be reused as a persist purpose.
///
/// # Errors
///
/// Returns [`AuthorizationError`] for tenant, kind, owner, or identity failure.
pub fn authorize_measurement_session(
    actor: &AuthorizationContext,
    session: &MeasurementSession,
) -> Result<(), AuthorizationError> {
    authorize_session_scope(
        actor,
        session.tenant_ref(),
        session.owner_participant_ref(),
        session.session_ref(),
    )
}

/// Authorize reload against a stored session header before ciphertext is opened.
///
/// # Errors
///
/// Returns [`AuthorizationError`] for tenant, kind, owner, or identity failure.
pub fn authorize_stored_measurement_session(
    actor: &AuthorizationContext,
    tenant_ref: &str,
    owner_participant_ref: &str,
    session_ref: &str,
) -> Result<(), AuthorizationError> {
    authorize_session_scope(actor, tenant_ref, owner_participant_ref, session_ref)
}

fn authorize_session_scope(
    actor: &AuthorizationContext,
    tenant_ref: &str,
    owner_participant_ref: &str,
    session_ref: &str,
) -> Result<(), AuthorizationError> {
    let resource = ResourceScope::participant_owned(
        ResourceKind::AssessmentSession,
        tenant_ref,
        owner_participant_ref,
        session_ref,
    )?;
    authorize(actor, &resource, ProductPermission::ManageOwnSession)
}

pub(crate) fn purpose_name(purpose: ConsentPurpose) -> &'static str {
    match purpose {
        ConsentPurpose::ServiceOperation => "service_operation",
        ConsentPurpose::AccountPersistence => "account_persistence",
        ConsentPurpose::LongitudinalObservation => "longitudinal_observation",
        ConsentPurpose::ResearchContribution => "research_contribution",
        ConsentPurpose::Communications => "communications",
    }
}

pub(crate) fn decision_name(decision: ConsentDecision) -> &'static str {
    match decision {
        ConsentDecision::Granted => "granted",
        ConsentDecision::Revoked => "revoked",
    }
}

pub(crate) fn parse_purpose(name: &str) -> Result<ConsentPurpose, MeasurementSessionError> {
    match name {
        "service_operation" => Ok(ConsentPurpose::ServiceOperation),
        "account_persistence" => Ok(ConsentPurpose::AccountPersistence),
        "longitudinal_observation" => Ok(ConsentPurpose::LongitudinalObservation),
        "research_contribution" => Ok(ConsentPurpose::ResearchContribution),
        "communications" => Ok(ConsentPurpose::Communications),
        _ => Err(MeasurementSessionError::SealingFailed),
    }
}

pub(crate) fn parse_decision(name: &str) -> Result<ConsentDecision, MeasurementSessionError> {
    match name {
        "granted" => Ok(ConsentDecision::Granted),
        "revoked" => Ok(ConsentDecision::Revoked),
        _ => Err(MeasurementSessionError::SealingFailed),
    }
}

fn associated_data(session_ref: &str, field_name: &str, event_ref: &str) -> String {
    format!("{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0{session_ref}\0{field_name}\0{event_ref}")
}

fn map_aead<T>(result: Result<T, aes_gcm::aead::Error>) -> Result<T, MeasurementSessionError> {
    result.map_err(|_| MeasurementSessionError::SealingFailed)
}

fn empty_ciphertext_on_infallible_encrypt(_: aes_gcm::aead::Error) -> Vec<u8> {
    Vec::new()
}

fn nonce_for(material: &str) -> [u8; 12] {
    let digest = Sha256::digest(material.as_bytes());
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&digest[..12]);
    nonce
}

fn push_field(bytes: &mut Vec<u8>, field: &str) {
    let field_bytes = field.as_bytes();
    let field_len = u64::try_from(field_bytes.len()).expect("field length fits a u64");
    bytes.extend_from_slice(&field_len.to_be_bytes());
    bytes.extend_from_slice(field_bytes);
}

fn required_reference(reference: &str) -> Result<&str, MeasurementSessionError> {
    normalized_reference(reference).ok_or(MeasurementSessionError::InvalidReference)
}

#[cfg(test)]
mod tests {
    use super::{
        authorize_measurement_session, authorize_stored_measurement_session, decision_name,
        empty_ciphertext_on_infallible_encrypt, map_aead, parse_decision, parse_purpose,
        purpose_name, ExportSnapshotPointer, MeasurementSession, MeasurementSessionError,
        MeasurementSessionInput, SealedPayload, SessionAuditEvent, SessionConsentRecord,
        SessionEncryptionKey, SessionMembership, MEASUREMENT_SESSION_PERSIST_PURPOSE,
    };
    use crate::authorization::{
        AuthorizationContext, AuthorizationError, ProductPermission, ProductRole, ResourceKind,
        ResourceScope,
    };
    use crate::consent::{ConsentDecision, ConsentPurpose};

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn key() -> SessionEncryptionKey {
        SessionEncryptionKey::new(MEASUREMENT_SESSION_PERSIST_PURPOSE, [7_u8; 32]).unwrap()
    }

    fn membership(participant_ref: &str) -> SessionMembership {
        SessionMembership::new(participant_ref, "tenant_alpha", 10, 20).unwrap()
    }

    fn grant(event_ref: &str, participant_ref: &str) -> SessionConsentRecord {
        SessionConsentRecord::new(
            event_ref,
            participant_ref,
            ConsentPurpose::ServiceOperation,
            ConsentDecision::Granted,
            "consent_form_v1",
            None,
            30,
        )
        .unwrap()
    }

    fn audit(event_ref: &str) -> SessionAuditEvent {
        SessionAuditEvent::new(
            event_ref,
            "actor_alpha",
            "session_persist",
            MEASUREMENT_SESSION_PERSIST_PURPOSE,
            DIGEST,
            40,
        )
        .unwrap()
    }

    fn pointer() -> ExportSnapshotPointer {
        ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", DIGEST, 50).unwrap()
    }

    fn session_input(
        created_at_unix_ms: u64,
        memberships: Vec<SessionMembership>,
        consent_records: Vec<SessionConsentRecord>,
        audit_events: Vec<SessionAuditEvent>,
        export_snapshot_pointer: Option<ExportSnapshotPointer>,
    ) -> MeasurementSessionInput {
        MeasurementSessionInput {
            session_ref: "session_alpha".to_owned(),
            tenant_ref: "tenant_alpha".to_owned(),
            owner_participant_ref: "participant_alpha".to_owned(),
            created_at_unix_ms,
            memberships,
            consent_records,
            audit_events,
            export_snapshot_pointer,
        }
    }

    fn session() -> MeasurementSession {
        MeasurementSession::new(session_input(
            5,
            vec![membership("participant_alpha")],
            vec![grant("consent_alpha", "participant_alpha")],
            vec![audit("audit_alpha")],
            Some(pointer()),
        ))
        .unwrap()
    }

    fn actor() -> AuthorizationContext {
        AuthorizationContext::new(
            "tenant_alpha",
            "subject_alpha",
            Some("participant_alpha"),
            &[ProductRole::Participant],
        )
        .unwrap()
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn constructors_and_accessors_cover_success_and_failure_paths() {
        let built = session();
        assert_eq!(key().purpose_ref(), MEASUREMENT_SESSION_PERSIST_PURPOSE);
        assert_eq!(built.session_ref(), "session_alpha");
        assert_eq!(built.tenant_ref(), "tenant_alpha");
        assert_eq!(built.owner_participant_ref(), "participant_alpha");
        assert_eq!(built.created_at_unix_ms(), 5);
        assert_eq!(
            built.memberships()[0].participant_ref(),
            "participant_alpha"
        );
        assert_eq!(built.memberships()[0].tenant_ref(), "tenant_alpha");
        assert_eq!(built.memberships()[0].created_at_unix_ms(), 10);
        assert_eq!(built.memberships()[0].enrolled_at_unix_ms(), 20);
        assert_eq!(built.consent_records()[0].event_ref(), "consent_alpha");
        assert_eq!(
            built.consent_records()[0].participant_ref(),
            "participant_alpha"
        );
        assert_eq!(
            built.consent_records()[0].purpose(),
            ConsentPurpose::ServiceOperation
        );
        assert_eq!(
            built.consent_records()[0].decision(),
            ConsentDecision::Granted
        );
        assert_eq!(
            built.consent_records()[0].consent_form_version_ref(),
            "consent_form_v1"
        );
        assert_eq!(built.consent_records()[0].research_scope_ref(), None);
        assert_eq!(built.consent_records()[0].decided_at_unix_ms(), 30);
        assert_eq!(built.audit_events()[0].event_ref(), "audit_alpha");
        assert_eq!(built.audit_events()[0].actor_ref(), "actor_alpha");
        assert_eq!(built.audit_events()[0].action_name(), "session_persist");
        assert_eq!(
            built.audit_events()[0].purpose_ref(),
            MEASUREMENT_SESSION_PERSIST_PURPOSE
        );
        assert_eq!(built.audit_events()[0].evidence_digest(), DIGEST);
        assert_eq!(built.audit_events()[0].occurred_at_unix_ms(), 40);
        let export_pointer = built.export_snapshot_pointer().unwrap();
        assert_eq!(export_pointer.snapshot_ref(), "snapshot_alpha");
        assert_eq!(export_pointer.request_ref(), "request_alpha");
        assert_eq!(export_pointer.content_digest(), DIGEST);
        assert_eq!(export_pointer.created_at_unix_ms(), 50);
        assert!(built.service_operation_is_granted("participant_alpha"));
        assert!(!built.service_operation_is_granted("participant_other"));
        assert_eq!(
            SessionEncryptionKey::new("12", [1_u8; 32]).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionEncryptionKey::new("export_delivery", [1_u8; 32]).unwrap_err(),
            MeasurementSessionError::InvalidEncryptionPurpose
        );
        assert_eq!(
            SessionMembership::new(" ", "tenant_alpha", 1, 1).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionMembership::new("participant_alpha", " ", 1, 1).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionMembership::new("participant_alpha", "tenant_alpha", 0, 1).unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        assert_eq!(
            SessionMembership::new("participant_alpha", "tenant_alpha", 1, 0).unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        assert_eq!(
            SessionConsentRecord::new(
                "consent_alpha",
                "participant_alpha",
                ConsentPurpose::ResearchContribution,
                ConsentDecision::Granted,
                "consent_form_v1",
                Some("research_scope_alpha"),
                0,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        assert_eq!(
            SessionConsentRecord::new(
                "12",
                "participant_alpha",
                ConsentPurpose::ServiceOperation,
                ConsentDecision::Granted,
                "consent_form_v1",
                None,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionConsentRecord::new(
                "consent_alpha",
                " ",
                ConsentPurpose::ServiceOperation,
                ConsentDecision::Granted,
                "consent_form_v1",
                None,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionConsentRecord::new(
                "consent_alpha",
                "participant_alpha",
                ConsentPurpose::ServiceOperation,
                ConsentDecision::Granted,
                " ",
                None,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionConsentRecord::new(
                "consent_alpha",
                "participant_alpha",
                ConsentPurpose::ResearchContribution,
                ConsentDecision::Granted,
                "consent_form_v1",
                Some(" "),
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        let research = SessionConsentRecord::new(
            "consent_research",
            "participant_alpha",
            ConsentPurpose::ResearchContribution,
            ConsentDecision::Granted,
            "consent_form_v1",
            Some("research_scope_alpha"),
            1,
        )
        .unwrap();
        assert_eq!(research.research_scope_ref(), Some("research_scope_alpha"));
        assert_eq!(
            SessionAuditEvent::new(
                "audit_alpha",
                "actor_alpha",
                "session_persist",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                "md5:00",
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidContentDigest
        );
        assert_eq!(
            SessionAuditEvent::new(
                " ",
                "actor_alpha",
                "session_persist",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                DIGEST,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionAuditEvent::new(
                "audit_alpha",
                "12",
                "session_persist",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                DIGEST,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionAuditEvent::new(
                "audit_alpha",
                "actor_alpha",
                " ",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                DIGEST,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionAuditEvent::new(
                "audit_alpha",
                "actor_alpha",
                "session_persist",
                "12",
                DIGEST,
                1,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            SessionAuditEvent::new(
                "audit_alpha",
                "actor_alpha",
                "session_persist",
                MEASUREMENT_SESSION_PERSIST_PURPOSE,
                DIGEST,
                0,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        assert_eq!(
            ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", DIGEST, 0).unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        assert_eq!(
            ExportSnapshotPointer::new(" ", "request_alpha", DIGEST, 1).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            ExportSnapshotPointer::new("snapshot_alpha", "12", DIGEST, 1).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            ExportSnapshotPointer::new("snapshot_alpha", "request_alpha", "sha256:zz", 1)
                .unwrap_err(),
            MeasurementSessionError::InvalidContentDigest
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                0,
                vec![membership("participant_alpha")],
                Vec::new(),
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::InvalidTimestamp
        );
        let mut invalid_session = session_input(
            1,
            vec![membership("participant_alpha")],
            Vec::new(),
            Vec::new(),
            None,
        );
        invalid_session.session_ref = "12".to_owned();
        assert_eq!(
            MeasurementSession::new(invalid_session).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        let mut invalid_tenant = session_input(
            1,
            vec![membership("participant_alpha")],
            Vec::new(),
            Vec::new(),
            None,
        );
        invalid_tenant.tenant_ref = " ".to_owned();
        assert_eq!(
            MeasurementSession::new(invalid_tenant).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        let mut invalid_owner = session_input(
            1,
            vec![membership("participant_alpha")],
            Vec::new(),
            Vec::new(),
            None,
        );
        invalid_owner.owner_participant_ref = "12".to_owned();
        assert_eq!(
            MeasurementSession::new(invalid_owner).unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            MeasurementSession::new(session_input(1, Vec::new(), Vec::new(), Vec::new(), None))
                .unwrap_err(),
            MeasurementSessionError::OwnerNotMember
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![membership("participant_beta")],
                Vec::new(),
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::OwnerNotMember
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![
                    membership("participant_alpha"),
                    membership("participant_alpha"),
                ],
                Vec::new(),
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::DuplicateMembership
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![SessionMembership::new("participant_alpha", "tenant_beta", 10, 20).unwrap()],
                Vec::new(),
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![membership("participant_alpha")],
                vec![
                    grant("consent_alpha", "participant_alpha"),
                    grant("consent_alpha", "participant_alpha"),
                ],
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::DuplicateEventIdentity
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![membership("participant_alpha")],
                vec![grant("consent_alpha", "participant_beta")],
                Vec::new(),
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::ConsentParticipantNotMember
        );
        assert_eq!(
            MeasurementSession::new(session_input(
                1,
                vec![membership("participant_alpha")],
                Vec::new(),
                vec![audit("audit_alpha"), audit("audit_alpha")],
                None,
            ))
            .unwrap_err(),
            MeasurementSessionError::DuplicateEventIdentity
        );
        let without_pointer = MeasurementSession::new(session_input(
            1,
            vec![membership("participant_alpha")],
            Vec::new(),
            Vec::new(),
            None,
        ))
        .unwrap();
        assert!(without_pointer.export_snapshot_pointer().is_none());
        assert_ne!(without_pointer.provenance_bytes(), built.provenance_bytes());
        assert_ne!(
            without_pointer.provenance_digest(),
            built.provenance_digest()
        );
    }

    #[test]
    fn sealing_round_trip_and_fail_closed_paths() {
        let encryption_key = key();
        assert_eq!(
            encryption_key.purpose_ref(),
            MEASUREMENT_SESSION_PERSIST_PURPOSE
        );
        let sealed = encryption_key
            .seal("nonce_material", "aad", "plaintext")
            .unwrap();
        assert_eq!(sealed.nonce().len(), 12);
        assert!(!sealed.ciphertext().is_empty());
        assert_eq!(encryption_key.open(&sealed, "aad").unwrap(), "plaintext");
        assert_eq!(
            encryption_key.open(&sealed, "other_aad").unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let other_key =
            SessionEncryptionKey::new(MEASUREMENT_SESSION_PERSIST_PURPOSE, [8_u8; 32]).unwrap();
        assert_eq!(
            other_key.open(&sealed, "aad").unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        assert_eq!(
            SealedPayload::from_stored(&[1_u8, 2], vec![3]).unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        assert_eq!(
            SealedPayload::from_stored(&[0_u8; 12], Vec::new()).unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let restored =
            SealedPayload::from_stored(sealed.nonce(), sealed.ciphertext().to_vec()).unwrap();
        assert_eq!(encryption_key.open(&restored, "aad").unwrap(), "plaintext");
        let built = session();
        let consent_sealed =
            built.consent_records()[0].sealed_payload(&encryption_key, built.session_ref());
        let opened_consent = SessionConsentRecord::from_sealed(
            "consent_alpha",
            "participant_alpha",
            &encryption_key,
            built.session_ref(),
            &consent_sealed,
        )
        .unwrap();
        assert_eq!(opened_consent, built.consent_records()[0]);
        let audit_sealed =
            built.audit_events()[0].sealed_payload(&encryption_key, built.session_ref());
        let opened_audit = SessionAuditEvent::from_sealed(
            "audit_alpha",
            "actor_alpha",
            40,
            &encryption_key,
            built.session_ref(),
            &audit_sealed,
        )
        .unwrap();
        assert_eq!(opened_audit, built.audit_events()[0]);
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_alpha",
                "participant_alpha",
                &encryption_key,
                "session_other",
                &consent_sealed,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        assert_eq!(
            SessionAuditEvent::from_sealed(
                "audit_alpha",
                "actor_alpha",
                40,
                &encryption_key,
                "session_other",
                &audit_sealed,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let damaged = SealedPayload::from_stored(&[9_u8; 12], vec![1; 32]).unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_alpha",
                "participant_alpha",
                &encryption_key,
                built.session_ref(),
                &damaged,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
    }

    #[test]
    fn purpose_and_decision_names_are_exhaustive() {
        for purpose in [
            ConsentPurpose::ServiceOperation,
            ConsentPurpose::AccountPersistence,
            ConsentPurpose::LongitudinalObservation,
            ConsentPurpose::ResearchContribution,
            ConsentPurpose::Communications,
        ] {
            assert_eq!(parse_purpose(purpose_name(purpose)).unwrap(), purpose);
        }
        assert_eq!(
            parse_purpose("score_kernel").unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        assert_eq!(
            parse_decision(decision_name(ConsentDecision::Granted)).unwrap(),
            ConsentDecision::Granted
        );
        assert_eq!(
            parse_decision(decision_name(ConsentDecision::Revoked)).unwrap(),
            ConsentDecision::Revoked
        );
        assert_eq!(
            parse_decision("scored").unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let revoked = SessionConsentRecord::new(
            "consent_revoked",
            "participant_alpha",
            ConsentPurpose::AccountPersistence,
            ConsentDecision::Revoked,
            "consent_form_v1",
            None,
            2,
        )
        .unwrap();
        assert_eq!(revoked.decision(), ConsentDecision::Revoked);
        assert_eq!(revoked.purpose(), ConsentPurpose::AccountPersistence);
        for (purpose, event_ref) in [
            (ConsentPurpose::LongitudinalObservation, "consent_long"),
            (ConsentPurpose::Communications, "consent_comms"),
        ] {
            let record = SessionConsentRecord::new(
                event_ref,
                "participant_alpha",
                purpose,
                ConsentDecision::Granted,
                "consent_form_v1",
                None,
                3,
            )
            .unwrap();
            assert_eq!(record.purpose(), purpose);
        }
    }

    #[test]
    fn authorization_is_purpose_limited_to_manage_own_session() {
        let built = session();
        authorize_measurement_session(&actor(), &built).unwrap();
        authorize_stored_measurement_session(
            &actor(),
            built.tenant_ref(),
            built.owner_participant_ref(),
            built.session_ref(),
        )
        .unwrap();
        let foreign = AuthorizationContext::new(
            "tenant_beta",
            "subject_alpha",
            Some("participant_alpha"),
            &[ProductRole::Participant],
        )
        .unwrap();
        assert_eq!(
            authorize_measurement_session(&foreign, &built).unwrap_err(),
            AuthorizationError::CrossTenantDenied
        );
        let other_owner = AuthorizationContext::new(
            "tenant_alpha",
            "subject_alpha",
            Some("participant_beta"),
            &[ProductRole::Participant],
        )
        .unwrap();
        assert_eq!(
            authorize_measurement_session(&other_owner, &built).unwrap_err(),
            AuthorizationError::OwnerMismatch
        );
        assert_eq!(
            authorize_stored_measurement_session(
                &actor(),
                "12",
                "participant_alpha",
                "session_alpha"
            )
            .unwrap_err(),
            AuthorizationError::InvalidReference
        );
        assert_eq!(
            authorize_stored_measurement_session(&actor(), "tenant_alpha", " ", "session_alpha")
                .unwrap_err(),
            AuthorizationError::InvalidReference
        );
        assert_eq!(
            authorize_stored_measurement_session(
                &actor(),
                "tenant_alpha",
                "participant_alpha",
                "12"
            )
            .unwrap_err(),
            AuthorizationError::InvalidReference
        );
        let consent_resource = ResourceScope::participant_owned(
            ResourceKind::ConsentLedger,
            "tenant_alpha",
            "participant_alpha",
            "ledger_alpha",
        )
        .unwrap();
        assert_eq!(
            crate::authorization::authorize(
                &actor(),
                &consent_resource,
                ProductPermission::ManageOwnSession,
            )
            .unwrap_err(),
            AuthorizationError::ResourceKindMismatch
        );
        for error in [
            MeasurementSessionError::InvalidReference,
            MeasurementSessionError::InvalidTimestamp,
            MeasurementSessionError::InvalidContentDigest,
            MeasurementSessionError::OwnerNotMember,
            MeasurementSessionError::DuplicateMembership,
            MeasurementSessionError::DuplicateEventIdentity,
            MeasurementSessionError::ConsentParticipantNotMember,
            MeasurementSessionError::InvalidEncryptionPurpose,
            MeasurementSessionError::SealingFailed,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn from_sealed_rejects_truncated_and_non_utf8_payloads() {
        let encryption_key = key();
        let truncated = encryption_key
            .seal(
                "session_alpha\0consent\0consent_alpha",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0consent_record\0consent_alpha"
                ),
                "service_operation",
            )
            .unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_alpha",
                "participant_alpha",
                &encryption_key,
                "session_alpha",
                &truncated,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let bad_decision = encryption_key
            .seal(
                "session_alpha\0consent\0consent_decision",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0consent_record\0consent_decision"
                ),
                "service_operation\u{1f}scored\u{1f}consent_form_v1\u{1f}\u{1f}1",
            )
            .unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_decision",
                "participant_alpha",
                &encryption_key,
                "session_alpha",
                &bad_decision,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let bad_purpose = encryption_key
            .seal(
                "session_alpha\0consent\0consent_purpose",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0consent_record\0consent_purpose"
                ),
                "score_kernel\u{1f}granted\u{1f}consent_form_v1\u{1f}\u{1f}1",
            )
            .unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_purpose",
                "participant_alpha",
                &encryption_key,
                "session_alpha",
                &bad_purpose,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let bad_time = encryption_key
            .seal(
                "session_alpha\0consent\0consent_time",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0consent_record\0consent_time"
                ),
                "service_operation\u{1f}granted\u{1f}consent_form_v1\u{1f}\u{1f}not-a-time",
            )
            .unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_time",
                "participant_alpha",
                &encryption_key,
                "session_alpha",
                &bad_time,
            )
            .unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        let empty_form = encryption_key
            .seal(
                "session_alpha\0consent\0consent_form",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0consent_record\0consent_form"
                ),
                "service_operation\u{1f}granted\u{1f}\u{1f}\u{1f}1",
            )
            .unwrap();
        assert_eq!(
            SessionConsentRecord::from_sealed(
                "consent_form",
                "participant_alpha",
                &encryption_key,
                "session_alpha",
                &empty_form,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidReference
        );
        let bad_audit = encryption_key
            .seal(
                "session_alpha\0audit\0audit_bad",
                &format!(
                    "{MEASUREMENT_SESSION_PERSIST_PURPOSE}\0session_alpha\0audit_event\0audit_bad"
                ),
                "session_persist\u{1f}measurement_session_persist\u{1f}md5:00",
            )
            .unwrap();
        assert_eq!(
            SessionAuditEvent::from_sealed(
                "audit_bad",
                "actor_alpha",
                40,
                &encryption_key,
                "session_alpha",
                &bad_audit,
            )
            .unwrap_err(),
            MeasurementSessionError::InvalidContentDigest
        );
        let raw = encryption_key.seal_bytes("nonce_raw", "aad_raw", &[0xff, 0xfe]);
        assert_eq!(
            encryption_key.open(&raw, "aad_raw").unwrap_err(),
            MeasurementSessionError::SealingFailed
        );
        assert!(map_aead::<()>(Err(aes_gcm::aead::Error)).is_err());
        assert_eq!(map_aead(Ok(7_u8)).unwrap(), 7_u8);
        assert!(empty_ciphertext_on_infallible_encrypt(aes_gcm::aead::Error).is_empty());
    }
}
