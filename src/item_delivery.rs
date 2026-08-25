//! Session-bound immutable item-delivery evidence.
//!
//! Item delivery is product-runtime state rather than psychometric item-selection
//! arithmetic. A ledger is created only from an already validated immutable
//! [`InstrumentReleaseManifest`], so the release identity, locale, content digest,
//! and complete allowed item-version set cannot drift apart. New logical delivery
//! events are accepted only while the assessment session is active; exact retries of
//! previously accepted delivery identities remain idempotent after lifecycle advance.

use crate::instrument::InstrumentReleaseManifest;
use crate::reference::normalized_reference;
use crate::session::SessionState;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Borrowed evidence required to record one item delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemDeliveryRequest<'a> {
    /// Opaque server-owned idempotency reference for this logical delivery.
    pub delivery_ref: &'a str,
    /// Exact immutable item-version reference presented to the participant.
    pub item_version_ref: &'a str,
    /// Versioned presentation-context reference governing how the item was shown.
    pub presentation_context_ref: &'a str,
    /// Optional evidence identifying a deterministic or adaptive selection decision.
    pub selection_evidence_ref: Option<&'a str>,
}

/// One accepted immutable item-delivery event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDeliveryEvent {
    delivery_ref: String,
    item_version_ref: String,
    presentation_context_ref: String,
    selection_evidence_ref: Option<String>,
    sequence: usize,
}

impl ItemDeliveryEvent {
    /// Return the opaque delivery idempotency reference.
    #[must_use]
    pub fn delivery_ref(&self) -> &str {
        &self.delivery_ref
    }

    /// Return the exact immutable item-version reference that was presented.
    #[must_use]
    pub fn item_version_ref(&self) -> &str {
        &self.item_version_ref
    }

    /// Return the versioned presentation-context reference.
    #[must_use]
    pub fn presentation_context_ref(&self) -> &str {
        &self.presentation_context_ref
    }

    /// Return optional evidence for the item-selection decision.
    #[must_use]
    pub fn selection_evidence_ref(&self) -> Option<&str> {
        self.selection_evidence_ref.as_deref()
    }

    /// Return the server-assigned monotonic delivery sequence.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }
}

/// Fail-closed item-delivery error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ItemDeliveryError {
    /// A required reference was blank, numeric-like, unsafe, or not already canonical.
    InvalidReference,
    /// A new logical delivery was attempted while the assessment session was not active.
    SessionNotActive(SessionState),
    /// A delivery reference was replayed with evidence different from its first use.
    IdempotencyConflict,
    /// The requested item version is not part of the exact immutable release manifest.
    ItemNotInRelease,
    /// The immutable item version had already been delivered in this session.
    DuplicateItemDelivery,
}

impl Display for ItemDeliveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReference => {
                formatter.write_str("item delivery references must be opaque non-numeric values")
            }
            Self::SessionNotActive(state) => {
                write!(
                    formatter,
                    "session {state:?} cannot deliver assessment items"
                )
            }
            Self::IdempotencyConflict => {
                formatter.write_str("delivery reference was already used for different evidence")
            }
            Self::ItemNotInRelease => formatter
                .write_str("item version is not part of the bound instrument release manifest"),
            Self::DuplicateItemDelivery => {
                formatter.write_str("item version was already delivered in this session")
            }
        }
    }
}

impl Error for ItemDeliveryError {}

/// Product-owned item-delivery ledger for one assessment session.
///
/// The ledger deliberately records item-delivery evidence only. Selection/calibration
/// algorithms remain in `fast-mlsirm`; persistence and transport adapters may store or
/// expose these values differently but must preserve the same idempotency, ordering,
/// exact-release membership, and active-session invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDeliveryLedger {
    session_ref: String,
    instrument_release_ref: String,
    release_content_digest: String,
    locale: String,
    allowed_item_version_refs: Vec<String>,
    events: Vec<ItemDeliveryEvent>,
}

impl ItemDeliveryLedger {
    /// Create an empty item-delivery ledger from one exact immutable release manifest.
    ///
    /// Release identity, content digest, locale, and allowed item versions are copied
    /// from the validated manifest as one unit. This prevents callers from composing
    /// an apparently valid ledger from references that belong to different releases.
    /// The session reference must already use its exact canonical spelling; this
    /// boundary rejects padded aliases instead of silently trimming them.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryError::InvalidReference`] when `session_ref` is blank,
    /// numeric-like, unsafe, or not already in canonical spelling.
    pub fn from_manifest(
        session_ref: &str,
        manifest: &InstrumentReleaseManifest,
    ) -> Result<Self, ItemDeliveryError> {
        let session_ref = required_reference(session_ref)?;
        Ok(Self {
            session_ref: session_ref.to_owned(),
            instrument_release_ref: manifest.release_ref().to_owned(),
            release_content_digest: manifest.content_digest().to_owned(),
            locale: manifest.locale().to_owned(),
            allowed_item_version_refs: manifest.item_version_refs().to_vec(),
            events: Vec::new(),
        })
    }

    /// Return the assessment-session reference bound to this ledger.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the immutable instrument-release reference bound to this ledger.
    #[must_use]
    pub fn instrument_release_ref(&self) -> &str {
        &self.instrument_release_ref
    }

    /// Return the canonical digest of the immutable release content.
    #[must_use]
    pub fn release_content_digest(&self) -> &str {
        &self.release_content_digest
    }

    /// Return the exact locale pinned for item delivery.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Return the exact ordered item-version set allowed by the release manifest.
    #[must_use]
    pub fn allowed_item_version_refs(&self) -> &[String] {
        &self.allowed_item_version_refs
    }

    /// Return the number of accepted logical item deliveries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Return whether this session has no accepted item deliveries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Return accepted item deliveries in server-authoritative order.
    #[must_use]
    pub fn events(&self) -> &[ItemDeliveryEvent] {
        &self.events
    }

    /// Record one item delivery or replay an identical accepted delivery.
    ///
    /// Exact replay of a previously accepted `delivery_ref` returns the original
    /// immutable event even after the session leaves [`SessionState::Active`]. Reuse
    /// of that identity with different evidence fails closed. Every genuinely new
    /// logical delivery still requires an active session and an item version present
    /// in the exact bound release manifest. A different delivery identity cannot
    /// re-administer an item version already delivered in the session. Request
    /// references must already be canonical, so whitespace-padded aliases cannot be
    /// rebound to an existing delivery, item, presentation, or selection identity.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryError::InvalidReference`] for malformed or non-canonical
    /// evidence, [`ItemDeliveryError::IdempotencyConflict`] for conflicting replay,
    /// [`ItemDeliveryError::SessionNotActive`] when a new logical delivery is offered
    /// outside an active session, [`ItemDeliveryError::ItemNotInRelease`] when the
    /// requested item is not present in the exact release manifest, or
    /// [`ItemDeliveryError::DuplicateItemDelivery`] when the same immutable item is
    /// offered under another logical delivery identity.
    pub fn deliver(
        &mut self,
        state: SessionState,
        request: ItemDeliveryRequest<'_>,
    ) -> Result<ItemDeliveryEvent, ItemDeliveryError> {
        let delivery_ref = required_reference(request.delivery_ref)?;
        let item_version_ref = required_reference(request.item_version_ref)?;
        let presentation_context_ref = required_reference(request.presentation_context_ref)?;
        let selection_evidence_ref = request
            .selection_evidence_ref
            .map(required_reference)
            .transpose()?;

        if let Some(existing) = self
            .events
            .iter()
            .find(|event| event.delivery_ref == delivery_ref)
        {
            return if existing.item_version_ref == item_version_ref
                && existing.presentation_context_ref == presentation_context_ref
                && existing.selection_evidence_ref.as_deref() == selection_evidence_ref
            {
                Ok(existing.clone())
            } else {
                Err(ItemDeliveryError::IdempotencyConflict)
            };
        }

        if state != SessionState::Active {
            return Err(ItemDeliveryError::SessionNotActive(state));
        }

        if !self
            .allowed_item_version_refs
            .iter()
            .any(|allowed| allowed == item_version_ref)
        {
            return Err(ItemDeliveryError::ItemNotInRelease);
        }

        if self
            .events
            .iter()
            .any(|event| event.item_version_ref == item_version_ref)
        {
            return Err(ItemDeliveryError::DuplicateItemDelivery);
        }

        let event = ItemDeliveryEvent {
            delivery_ref: delivery_ref.to_owned(),
            item_version_ref: item_version_ref.to_owned(),
            presentation_context_ref: presentation_context_ref.to_owned(),
            selection_evidence_ref: selection_evidence_ref.map(str::to_owned),
            sequence: self.events.len() + 1,
        };
        self.events.push(event.clone());
        Ok(event)
    }
}

fn required_reference(reference: &str) -> Result<&str, ItemDeliveryError> {
    let normalized = normalized_reference(reference).ok_or(ItemDeliveryError::InvalidReference)?;
    if normalized != reference {
        return Err(ItemDeliveryError::InvalidReference);
    }
    Ok(normalized)
}
