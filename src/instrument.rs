//! Immutable instrument-release manifest and publication lifecycle.
//!
//! Psychometrics Commons publishes assessment forms as immutable, version-pinned
//! release manifests. Publication state only controls whether the exact immutable
//! release may begin new sessions. Any content, scoring, norm, narrative, consent,
//! locale, or intended-use change requires a new release manifest rather than a
//! mutation of an already published artifact.

use crate::reference::normalized_reference;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Server-authoritative lifecycle state for one immutable instrument release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationState {
    /// The release is being assembled and cannot accept assessment sessions.
    Draft,
    /// The release awaits scientific, governance, or product approval.
    Review,
    /// The exact immutable release may accept new assessment sessions.
    Published,
    /// New sessions are temporarily blocked without changing the release manifest.
    Suspended,
    /// New sessions are permanently blocked for this release version.
    Retired,
}

impl PublicationState {
    /// Return whether this exact release may accept new assessment sessions.
    #[must_use]
    pub const fn accepts_new_sessions(self) -> bool {
        matches!(self, Self::Published)
    }

    /// Return whether this lifecycle state is permanently terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Retired)
    }
}

/// Command accepted by the instrument-publication lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationCommand {
    /// Submit the immutable draft manifest for review.
    SubmitReview,
    /// Publish a reviewed immutable release.
    Publish,
    /// Temporarily block new sessions for a published release.
    Suspend,
    /// Resume the same suspended immutable release.
    Reactivate,
    /// Permanently block new sessions for this release version.
    Retire,
}

/// Policy-evaluated status of scientific publication evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PublicationEvidenceStatus {
    /// Every mandatory publication gate represented by this record passed.
    Approved,
    /// At least one mandatory publication gate failed.
    Failed,
    /// One or more mandatory gates are unresolved, unavailable, or not evaluated.
    Unknown,
}

/// Fail-closed error returned by instrument-release operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InstrumentReleaseError {
    /// A required reference was blank or numeric-like instead of opaque.
    InvalidReference,
    /// The release has no item versions.
    EmptyItemSet,
    /// The release repeats an item-version reference.
    DuplicateItemReference,
    /// The locale is not a supported BCP 47-style tag.
    InvalidLocale,
    /// The release content digest is not a canonical SHA-256 digest reference.
    InvalidDigest,
    /// The evidence artifact digest is not a canonical SHA-256 digest reference.
    InvalidEvidenceDigest,
    /// The evidence validity window ends before its evaluation time.
    InvalidEvidenceWindow,
    /// An approved evidence record omitted mandatory evidence categories.
    IncompletePublicationEvidence,
    /// Publication evidence does not bind the exact immutable release bundle.
    PublicationEvidenceMismatch,
    /// No publication evidence record is bound to the reviewed release.
    MissingPublicationEvidence,
    /// The bound publication evidence is failed or unresolved.
    PublicationEvidenceNotApproved,
    /// The approved evidence is not effective at the server-authoritative publish time.
    PublicationEvidenceNotEffective,
    /// A server-authoritative timestamp was zero.
    InvalidTimestamp,
    /// An event timestamp moved backwards.
    NonMonotonicTimestamp,
    /// An event reference was replayed with different command evidence.
    ConflictingReplay,
    /// The requested command is not legal from the current publication state.
    InvalidTransition,
}

impl Display for InstrumentReleaseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReference => {
                "instrument release references must be opaque non-numeric values"
            }
            Self::EmptyItemSet => "instrument release must contain at least one item version",
            Self::DuplicateItemReference => {
                "instrument release item-version references must be unique"
            }
            Self::InvalidLocale => "instrument release locale must be a valid BCP 47-style tag",
            Self::InvalidDigest => {
                "instrument release content digest must be sha256 followed by 64 lowercase hexadecimal digits"
            }
            Self::InvalidEvidenceDigest => {
                "publication evidence digest must be sha256 followed by 64 lowercase hexadecimal digits"
            }
            Self::InvalidEvidenceWindow => {
                "publication evidence validity must not end before its evaluation time"
            }
            Self::IncompletePublicationEvidence => {
                "approved publication evidence must include content or rights, scientific, and approval references"
            }
            Self::PublicationEvidenceMismatch => {
                "publication evidence must match the exact immutable instrument release bundle"
            }
            Self::MissingPublicationEvidence => {
                "reviewed instrument release requires bound publication evidence before publication"
            }
            Self::PublicationEvidenceNotApproved => {
                "instrument publication evidence must be policy-approved before publication"
            }
            Self::PublicationEvidenceNotEffective => {
                "approved publication evidence must be effective at the publication time"
            }
            Self::InvalidTimestamp => "instrument publication timestamps must be greater than zero",
            Self::NonMonotonicTimestamp => {
                "instrument publication event time must not move backwards"
            }
            Self::ConflictingReplay => {
                "instrument publication event reference was replayed with conflicting evidence"
            }
            Self::InvalidTransition => {
                "instrument publication command is not allowed from the current state"
            }
        })
    }
}

impl Error for InstrumentReleaseError {}

/// Immutable release-critical references for one locale-specific instrument version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentReleaseManifest {
    release_ref: String,
    instrument_ref: String,
    instrument_version_ref: String,
    construct_ref: String,
    item_version_refs: Vec<String>,
    locale: String,
    assessment_spec_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    narrative_version_ref: String,
    consent_requirement_refs: Vec<String>,
    intended_use_ref: String,
    limitations_ref: String,
    content_digest: String,
}

impl InstrumentReleaseManifest {
    /// Create a fully pinned immutable instrument-release manifest.
    ///
    /// Item order is semantically significant and is therefore preserved exactly.
    /// All public references must be opaque. The content digest identifies the
    /// canonical release bytes independently from human-readable version names.
    ///
    /// # Errors
    ///
    /// Returns an [`InstrumentReleaseError`] when a reference, locale, digest, or
    /// item-version set violates the publication contract.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        release_ref: &str,
        instrument_ref: &str,
        instrument_version_ref: &str,
        construct_ref: &str,
        item_version_refs: &[&str],
        locale: &str,
        assessment_spec_ref: &str,
        scoring_version_ref: &str,
        calibration_reference: &str,
        norm_version_ref: Option<&str>,
        narrative_version_ref: &str,
        consent_requirement_refs: &[&str],
        intended_use_ref: &str,
        limitations_ref: &str,
        content_digest: &str,
    ) -> Result<Self, InstrumentReleaseError> {
        if item_version_refs.is_empty() {
            return Err(InstrumentReleaseError::EmptyItemSet);
        }
        let item_version_refs = normalize_unique_references(
            item_version_refs,
            InstrumentReleaseError::DuplicateItemReference,
        )?;
        let consent_requirement_refs = normalize_unique_references(
            consent_requirement_refs,
            InstrumentReleaseError::InvalidReference,
        )?;
        let locale = locale.trim();
        if !valid_locale(locale) {
            return Err(InstrumentReleaseError::InvalidLocale);
        }
        if !valid_sha256_digest(content_digest) {
            return Err(InstrumentReleaseError::InvalidDigest);
        }
        let norm_version_ref = norm_version_ref
            .map(required_reference)
            .transpose()?
            .map(str::to_owned);
        let release_ref = required_reference(release_ref)?.to_owned();
        let instrument_ref = required_reference(instrument_ref)?.to_owned();
        let instrument_version_ref = required_reference(instrument_version_ref)?.to_owned();
        let construct_ref = required_reference(construct_ref)?.to_owned();
        let assessment_spec_ref = required_reference(assessment_spec_ref)?.to_owned();
        let scoring_version_ref = required_reference(scoring_version_ref)?.to_owned();
        let calibration_reference = required_reference(calibration_reference)?.to_owned();
        let narrative_version_ref = required_reference(narrative_version_ref)?.to_owned();
        let intended_use_ref = required_reference(intended_use_ref)?.to_owned();
        let limitations_ref = required_reference(limitations_ref)?.to_owned();

        Ok(Self {
            release_ref,
            instrument_ref,
            instrument_version_ref,
            construct_ref,
            item_version_refs,
            locale: locale.to_owned(),
            assessment_spec_ref,
            scoring_version_ref,
            calibration_reference,
            norm_version_ref,
            narrative_version_ref,
            consent_requirement_refs,
            intended_use_ref,
            limitations_ref,
            content_digest: content_digest.to_owned(),
        })
    }

    /// Return the opaque release reference.
    #[must_use]
    pub fn release_ref(&self) -> &str {
        &self.release_ref
    }

    /// Return the stable instrument family reference.
    #[must_use]
    pub fn instrument_ref(&self) -> &str {
        &self.instrument_ref
    }

    /// Return the exact instrument-version reference.
    #[must_use]
    pub fn instrument_version_ref(&self) -> &str {
        &self.instrument_version_ref
    }

    /// Return the construct definition reference.
    #[must_use]
    pub fn construct_ref(&self) -> &str {
        &self.construct_ref
    }

    /// Return the ordered immutable item-version references.
    #[must_use]
    pub fn item_version_refs(&self) -> &[String] {
        &self.item_version_refs
    }

    /// Return the locale pinned by the release.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the `AssessmentSpec` reference.
    #[must_use]
    pub fn assessment_spec_ref(&self) -> &str {
        &self.assessment_spec_ref
    }

    /// Return the scoring-version reference.
    #[must_use]
    pub fn scoring_version_ref(&self) -> &str {
        &self.scoring_version_ref
    }

    /// Return the calibration artifact reference.
    #[must_use]
    pub fn calibration_reference(&self) -> &str {
        &self.calibration_reference
    }

    /// Return the optional norm-version reference.
    #[must_use]
    pub fn norm_version_ref(&self) -> Option<&str> {
        self.norm_version_ref.as_deref()
    }

    /// Return the narrative-rule version reference.
    #[must_use]
    pub fn narrative_version_ref(&self) -> &str {
        &self.narrative_version_ref
    }

    /// Return the purpose-specific consent requirement references.
    #[must_use]
    pub fn consent_requirement_refs(&self) -> &[String] {
        &self.consent_requirement_refs
    }

    /// Return the intended-use metadata reference.
    #[must_use]
    pub fn intended_use_ref(&self) -> &str {
        &self.intended_use_ref
    }

    /// Return the limitations metadata reference.
    #[must_use]
    pub fn limitations_ref(&self) -> &str {
        &self.limitations_ref
    }

    /// Return the canonical content digest.
    #[must_use]
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
}

/// Immutable provenance and validity scope for a policy-evaluated evidence artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationEvidenceProvenance {
    evidence_digest: String,
    population_context_ref: String,
    administration_mode_ref: String,
    measurement_model_ref: String,
    evaluated_at_unix_ms: u64,
    valid_until_unix_ms: Option<u64>,
}

impl PublicationEvidenceProvenance {
    /// Create version-bound provenance for scientific publication evidence.
    ///
    /// The digest identifies the evidence artifact independently of release bytes.
    /// Population, administration, and measurement-model references remain opaque
    /// product evidence references; Psychometrics Commons does not recompute the
    /// upstream scientific evidence represented by them.
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentReleaseError::InvalidEvidenceDigest`] for a malformed
    /// evidence digest, [`InstrumentReleaseError::InvalidReference`] for malformed
    /// scope references, [`InstrumentReleaseError::InvalidTimestamp`] for a zero
    /// evaluation time, and [`InstrumentReleaseError::InvalidEvidenceWindow`] when
    /// validity ends before evaluation.
    pub fn new(
        evidence_digest: &str,
        population_context_ref: &str,
        administration_mode_ref: &str,
        measurement_model_ref: &str,
        evaluated_at_unix_ms: u64,
        valid_until_unix_ms: Option<u64>,
    ) -> Result<Self, InstrumentReleaseError> {
        if !valid_sha256_digest(evidence_digest) {
            return Err(InstrumentReleaseError::InvalidEvidenceDigest);
        }
        if evaluated_at_unix_ms == 0 {
            return Err(InstrumentReleaseError::InvalidTimestamp);
        }
        if valid_until_unix_ms.is_some_and(|valid_until| valid_until < evaluated_at_unix_ms) {
            return Err(InstrumentReleaseError::InvalidEvidenceWindow);
        }

        Ok(Self {
            evidence_digest: evidence_digest.to_owned(),
            population_context_ref: required_reference(population_context_ref)?.to_owned(),
            administration_mode_ref: required_reference(administration_mode_ref)?.to_owned(),
            measurement_model_ref: required_reference(measurement_model_ref)?.to_owned(),
            evaluated_at_unix_ms,
            valid_until_unix_ms,
        })
    }

    /// Return the canonical digest of the evaluated evidence artifact.
    #[must_use]
    pub fn evidence_digest(&self) -> &str {
        &self.evidence_digest
    }

    /// Return the population context reference used by the evidence policy.
    #[must_use]
    pub fn population_context_ref(&self) -> &str {
        &self.population_context_ref
    }

    /// Return the administration-mode reference used by the evidence policy.
    #[must_use]
    pub fn administration_mode_ref(&self) -> &str {
        &self.administration_mode_ref
    }

    /// Return the measurement-model reference represented by the evidence.
    #[must_use]
    pub fn measurement_model_ref(&self) -> &str {
        &self.measurement_model_ref
    }

    /// Return the server-authoritative evidence evaluation time.
    #[must_use]
    pub const fn evaluated_at_unix_ms(&self) -> u64 {
        self.evaluated_at_unix_ms
    }

    /// Return the inclusive evidence-validity deadline, when one exists.
    #[must_use]
    pub const fn valid_until_unix_ms(&self) -> Option<u64> {
        self.valid_until_unix_ms
    }

    fn is_effective_at(&self, occurred_at_unix_ms: u64) -> bool {
        if occurred_at_unix_ms < self.evaluated_at_unix_ms {
            return false;
        }
        self.valid_until_unix_ms
            .is_none_or(|valid_until| occurred_at_unix_ms <= valid_until)
    }
}

/// Immutable, policy-evaluated evidence bound to one exact publication bundle.
///
/// Psychometrics Commons stores only references to scientific and rights evidence.
/// It never recomputes upstream psychometric evidence in this publication boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationEvidenceRecord {
    publication_evidence_ref: String,
    evidence_policy_ref: String,
    release_ref: String,
    instrument_version_ref: String,
    item_version_refs: Vec<String>,
    content_digest: String,
    locale: String,
    intended_use_ref: String,
    assessment_spec_ref: String,
    scoring_version_ref: String,
    calibration_reference: String,
    norm_version_ref: Option<String>,
    limitations_ref: String,
    provenance: PublicationEvidenceProvenance,
    content_rights_evidence_refs: Vec<String>,
    scientific_evidence_refs: Vec<String>,
    approval_refs: Vec<String>,
    status: PublicationEvidenceStatus,
}

impl PublicationEvidenceRecord {
    /// Create one immutable policy-evaluated publication evidence record.
    ///
    /// `Approved` evidence must include content/rights, scientific, and approval
    /// references. Failed or unresolved evaluations may leave those collections
    /// empty so the product can retain explicit fail-closed evidence. Provenance is
    /// mandatory for every status so all decisions retain the evaluated context and
    /// evidence artifact identity.
    ///
    /// # Errors
    ///
    /// Returns an [`InstrumentReleaseError`] when structural references, locale,
    /// digest, or evidence collections are invalid or an approved record omits a
    /// mandatory evidence category.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        publication_evidence_ref: &str,
        evidence_policy_ref: &str,
        release_ref: &str,
        instrument_version_ref: &str,
        item_version_refs: &[&str],
        content_digest: &str,
        locale: &str,
        intended_use_ref: &str,
        assessment_spec_ref: &str,
        scoring_version_ref: &str,
        calibration_reference: &str,
        norm_version_ref: Option<&str>,
        limitations_ref: &str,
        provenance: PublicationEvidenceProvenance,
        content_rights_evidence_refs: &[&str],
        scientific_evidence_refs: &[&str],
        approval_refs: &[&str],
        status: PublicationEvidenceStatus,
    ) -> Result<Self, InstrumentReleaseError> {
        if item_version_refs.is_empty() {
            return Err(InstrumentReleaseError::EmptyItemSet);
        }
        let item_version_refs = normalize_unique_references(
            item_version_refs,
            InstrumentReleaseError::DuplicateItemReference,
        )?;
        let content_rights_evidence_refs = normalize_unique_references(
            content_rights_evidence_refs,
            InstrumentReleaseError::InvalidReference,
        )?;
        let scientific_evidence_refs = normalize_unique_references(
            scientific_evidence_refs,
            InstrumentReleaseError::InvalidReference,
        )?;
        let approval_refs =
            normalize_unique_references(approval_refs, InstrumentReleaseError::InvalidReference)?;
        let locale = locale.trim();
        if !valid_locale(locale) {
            return Err(InstrumentReleaseError::InvalidLocale);
        }
        if !valid_sha256_digest(content_digest) {
            return Err(InstrumentReleaseError::InvalidDigest);
        }
        if status == PublicationEvidenceStatus::Approved
            && (content_rights_evidence_refs.is_empty()
                || scientific_evidence_refs.is_empty()
                || approval_refs.is_empty())
        {
            return Err(InstrumentReleaseError::IncompletePublicationEvidence);
        }

        Ok(Self {
            publication_evidence_ref: required_reference(publication_evidence_ref)?.to_owned(),
            evidence_policy_ref: required_reference(evidence_policy_ref)?.to_owned(),
            release_ref: required_reference(release_ref)?.to_owned(),
            instrument_version_ref: required_reference(instrument_version_ref)?.to_owned(),
            item_version_refs,
            content_digest: content_digest.to_owned(),
            locale: locale.to_owned(),
            intended_use_ref: required_reference(intended_use_ref)?.to_owned(),
            assessment_spec_ref: required_reference(assessment_spec_ref)?.to_owned(),
            scoring_version_ref: required_reference(scoring_version_ref)?.to_owned(),
            calibration_reference: required_reference(calibration_reference)?.to_owned(),
            norm_version_ref: norm_version_ref
                .map(required_reference)
                .transpose()?
                .map(str::to_owned),
            limitations_ref: required_reference(limitations_ref)?.to_owned(),
            provenance,
            content_rights_evidence_refs,
            scientific_evidence_refs,
            approval_refs,
            status,
        })
    }

    /// Return the immutable publication-evidence reference.
    #[must_use]
    pub fn publication_evidence_ref(&self) -> &str {
        &self.publication_evidence_ref
    }

    /// Return the policy version that evaluated this evidence.
    #[must_use]
    pub fn evidence_policy_ref(&self) -> &str {
        &self.evidence_policy_ref
    }

    /// Return the immutable provenance and validity scope for this evidence.
    #[must_use]
    pub const fn provenance(&self) -> &PublicationEvidenceProvenance {
        &self.provenance
    }

    /// Return the content/rights evidence references.
    #[must_use]
    pub fn content_rights_evidence_refs(&self) -> &[String] {
        &self.content_rights_evidence_refs
    }

    /// Return the scientific evidence references consumed by the policy.
    #[must_use]
    pub fn scientific_evidence_refs(&self) -> &[String] {
        &self.scientific_evidence_refs
    }

    /// Return the approval evidence references.
    #[must_use]
    pub fn approval_refs(&self) -> &[String] {
        &self.approval_refs
    }

    /// Return the fail-closed policy evaluation status.
    #[must_use]
    pub const fn status(&self) -> PublicationEvidenceStatus {
        self.status
    }

    fn matches_manifest(&self, manifest: &InstrumentReleaseManifest) -> bool {
        (
            self.release_ref.as_str(),
            self.instrument_version_ref.as_str(),
            self.item_version_refs.as_slice(),
            self.content_digest.as_str(),
            self.locale.as_str(),
            self.intended_use_ref.as_str(),
            self.assessment_spec_ref.as_str(),
            self.scoring_version_ref.as_str(),
            self.calibration_reference.as_str(),
            self.norm_version_ref.as_deref(),
            self.limitations_ref.as_str(),
        ) == (
            manifest.release_ref(),
            manifest.instrument_version_ref(),
            manifest.item_version_refs(),
            manifest.content_digest(),
            manifest.locale(),
            manifest.intended_use_ref(),
            manifest.assessment_spec_ref(),
            manifest.scoring_version_ref(),
            manifest.calibration_reference(),
            manifest.norm_version_ref(),
            manifest.limitations_ref(),
        )
    }
}

/// Durable idempotency evidence for one accepted publication command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationEvent {
    event_ref: String,
    command: PublicationCommand,
    occurred_at_unix_ms: u64,
    publication_evidence_ref: Option<String>,
    evidence_policy_ref: Option<String>,
    publication_evidence_digest: Option<String>,
}

impl PublicationEvent {
    /// Return the opaque event reference used as the idempotency key.
    #[must_use]
    pub fn event_ref(&self) -> &str {
        &self.event_ref
    }

    /// Return the accepted publication command.
    #[must_use]
    pub const fn command(&self) -> PublicationCommand {
        self.command
    }

    /// Return the server-authoritative event time.
    #[must_use]
    pub const fn occurred_at_unix_ms(&self) -> u64 {
        self.occurred_at_unix_ms
    }

    /// Return the publication-evidence identity bound to a publish or reactivation event.
    #[must_use]
    pub fn publication_evidence_ref(&self) -> Option<&str> {
        self.publication_evidence_ref.as_deref()
    }

    /// Return the evidence-policy identity bound to a publish or reactivation event.
    #[must_use]
    pub fn evidence_policy_ref(&self) -> Option<&str> {
        self.evidence_policy_ref.as_deref()
    }

    /// Return the evidence-artifact digest bound to a publish or reactivation event.
    #[must_use]
    pub fn publication_evidence_digest(&self) -> Option<&str> {
        self.publication_evidence_digest.as_deref()
    }
}

/// Product-owned publication lifecycle for one immutable instrument-release manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentRelease {
    manifest: InstrumentReleaseManifest,
    publication_evidence: Option<PublicationEvidenceRecord>,
    state: PublicationState,
    created_at_unix_ms: u64,
    latest_event_at_unix_ms: u64,
    events: Vec<PublicationEvent>,
}

impl InstrumentRelease {
    /// Create a draft release around an immutable manifest.
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentReleaseError::InvalidTimestamp`] when the server creation
    /// time is zero.
    pub fn new(
        manifest: InstrumentReleaseManifest,
        created_at_unix_ms: u64,
    ) -> Result<Self, InstrumentReleaseError> {
        if created_at_unix_ms == 0 {
            return Err(InstrumentReleaseError::InvalidTimestamp);
        }
        Ok(Self {
            manifest,
            publication_evidence: None,
            state: PublicationState::Draft,
            created_at_unix_ms,
            latest_event_at_unix_ms: created_at_unix_ms,
            events: Vec::new(),
        })
    }

    /// Return the immutable release manifest.
    #[must_use]
    pub const fn manifest(&self) -> &InstrumentReleaseManifest {
        &self.manifest
    }

    /// Return the exact publication evidence currently bound to the release.
    #[must_use]
    pub const fn publication_evidence(&self) -> Option<&PublicationEvidenceRecord> {
        self.publication_evidence.as_ref()
    }

    /// Bind policy-evaluated evidence to the exact immutable release under review.
    ///
    /// Binding is deliberately separate from the `Publish` command so evidence may
    /// be inspected before publication. Rebinding is permitted only while the
    /// release remains in `Review`; a publish event records which current immutable
    /// evidence record justified the transition.
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentReleaseError::InvalidTransition`] outside `Review` and
    /// [`InstrumentReleaseError::PublicationEvidenceMismatch`] when the record does
    /// not bind the exact release version, item set, digest, locale, intended use,
    /// scoring, calibration, norm, and limitations.
    pub fn bind_publication_evidence(
        &mut self,
        publication_evidence: PublicationEvidenceRecord,
    ) -> Result<(), InstrumentReleaseError> {
        if self.state != PublicationState::Review {
            return Err(InstrumentReleaseError::InvalidTransition);
        }
        if !publication_evidence.matches_manifest(&self.manifest) {
            return Err(InstrumentReleaseError::PublicationEvidenceMismatch);
        }
        self.publication_evidence = Some(publication_evidence);
        Ok(())
    }

    /// Return the current publication state.
    #[must_use]
    pub const fn state(&self) -> PublicationState {
        self.state
    }

    /// Return the server-authoritative creation time.
    #[must_use]
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }

    /// Return accepted publication events in server-authoritative order.
    #[must_use]
    pub fn events(&self) -> &[PublicationEvent] {
        &self.events
    }

    /// Return whether this release may begin new assessment sessions.
    #[must_use]
    pub const fn accepts_new_sessions(&self) -> bool {
        self.state.accepts_new_sessions()
    }

    /// Apply one publication command using `event_ref` as its idempotency key.
    ///
    /// Exact replay of a previously accepted event returns the current state without
    /// reopening or rewinding a later lifecycle state. Reuse of an event reference
    /// with different command evidence fails closed. Both `Review -> Published` and
    /// `Suspended -> Published` additionally require exact, policy-approved publication
    /// evidence that is effective at the server-authoritative transition time. The
    /// accepted publish/reactivation event durably binds evidence identity,
    /// evidence-policy identity, and evidence-artifact digest.
    ///
    /// # Errors
    ///
    /// Returns an [`InstrumentReleaseError`] for invalid references/timestamps,
    /// conflicting replays, backward event time, missing/failed/stale publication
    /// evidence, or undocumented transitions.
    pub fn apply_command(
        &mut self,
        event_ref: &str,
        command: PublicationCommand,
        occurred_at_unix_ms: u64,
    ) -> Result<PublicationState, InstrumentReleaseError> {
        let event_ref = required_reference(event_ref)?;
        if let Some(existing) = self
            .events
            .iter()
            .find(|event| event.event_ref == event_ref)
        {
            return if existing.command == command
                && existing.occurred_at_unix_ms == occurred_at_unix_ms
            {
                Ok(self.state)
            } else {
                Err(InstrumentReleaseError::ConflictingReplay)
            };
        }
        if occurred_at_unix_ms == 0 {
            return Err(InstrumentReleaseError::InvalidTimestamp);
        }
        if occurred_at_unix_ms < self.latest_event_at_unix_ms {
            return Err(InstrumentReleaseError::NonMonotonicTimestamp);
        }

        let requires_publication_evidence = matches!(
            (self.state, command),
            (PublicationState::Review, PublicationCommand::Publish)
                | (PublicationState::Suspended, PublicationCommand::Reactivate)
        );
        let publication_binding = if requires_publication_evidence {
            let evidence = self
                .publication_evidence
                .as_ref()
                .ok_or(InstrumentReleaseError::MissingPublicationEvidence)?;
            if evidence.status() != PublicationEvidenceStatus::Approved {
                return Err(InstrumentReleaseError::PublicationEvidenceNotApproved);
            }
            if !evidence.provenance().is_effective_at(occurred_at_unix_ms) {
                return Err(InstrumentReleaseError::PublicationEvidenceNotEffective);
            }
            Some((
                evidence.publication_evidence_ref().to_owned(),
                evidence.evidence_policy_ref().to_owned(),
                evidence.provenance().evidence_digest().to_owned(),
            ))
        } else {
            None
        };

        let next = transition(self.state, command)?;
        let (publication_evidence_ref, evidence_policy_ref, publication_evidence_digest) =
            publication_binding.map_or(
                (None, None, None),
                |(evidence_ref, policy_ref, evidence_digest)| {
                    (Some(evidence_ref), Some(policy_ref), Some(evidence_digest))
                },
            );
        self.events.push(PublicationEvent {
            event_ref: event_ref.to_owned(),
            command,
            occurred_at_unix_ms,
            publication_evidence_ref,
            evidence_policy_ref,
            publication_evidence_digest,
        });
        self.state = next;
        self.latest_event_at_unix_ms = occurred_at_unix_ms;
        Ok(next)
    }
}

const fn transition(
    state: PublicationState,
    command: PublicationCommand,
) -> Result<PublicationState, InstrumentReleaseError> {
    use PublicationCommand::{Publish, Reactivate, Retire, SubmitReview, Suspend};
    use PublicationState::{Draft, Published, Review, Suspended};

    match (state, command) {
        (Draft, SubmitReview) => Ok(Review),
        (Review, Publish) | (Suspended, Reactivate) => Ok(Published),
        (Published, Suspend) => Ok(Suspended),
        (Published | Suspended, Retire) => Ok(PublicationState::Retired),
        _ => Err(InstrumentReleaseError::InvalidTransition),
    }
}

fn normalize_unique_references(
    references: &[&str],
    duplicate_error: InstrumentReleaseError,
) -> Result<Vec<String>, InstrumentReleaseError> {
    let mut normalized = Vec::with_capacity(references.len());
    for reference in references {
        let reference = required_reference(reference)?;
        if normalized.iter().any(|existing| existing == reference) {
            return Err(duplicate_error);
        }
        normalized.push(reference.to_owned());
    }
    Ok(normalized)
}

fn required_reference(reference: &str) -> Result<&str, InstrumentReleaseError> {
    normalized_reference(reference).ok_or(InstrumentReleaseError::InvalidReference)
}

fn valid_sha256_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_locale(locale: &str) -> bool {
    let mut subtags = locale.split('-');
    let primary = subtags.next().unwrap_or_default();
    if !(2..=8).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    subtags.all(|subtag| {
        (1..=8).contains(&subtag.len()) && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}
