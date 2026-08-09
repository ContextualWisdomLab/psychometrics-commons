//! Session-bound immutable item-delivery evidence.
//!
//! Item delivery is product-runtime state rather than psychometric item-selection
//! arithmetic. The ledger binds one assessment session to one immutable instrument
//! release, its canonical content digest, and one exact locale. Delivery events are
//! server-ordered, idempotent by opaque delivery reference, and accepted only while
//! the assessment session is active.

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
    /// A required reference was blank or numeric-like instead of opaque.
    InvalidReference,
    /// The pinned instrument-release digest was not canonical SHA-256.
    InvalidDigest,
    /// The pinned locale was not a valid BCP 47-style tag.
    InvalidLocale,
    /// The assessment session was not active when delivery was attempted.
    SessionNotActive(SessionState),
    /// A delivery reference was replayed with evidence different from its first use.
    IdempotencyConflict,
    /// The immutable item version had already been delivered in this session.
    DuplicateItemDelivery,
}

impl Display for ItemDeliveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReference => {
                formatter.write_str("item delivery references must be opaque non-numeric values")
            }
            Self::InvalidDigest => {
                formatter.write_str("item delivery release digest must be canonical sha256")
            }
            Self::InvalidLocale => {
                formatter.write_str("item delivery locale must be a valid BCP 47-style tag")
            }
            Self::SessionNotActive(state) => {
                write!(formatter, "session {state:?} cannot deliver assessment items")
            }
            Self::IdempotencyConflict => formatter
                .write_str("delivery reference was already used for different evidence"),
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
/// release-binding, and active-session invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDeliveryLedger {
    session_ref: String,
    instrument_release_ref: String,
    release_content_digest: String,
    locale: String,
    events: Vec<ItemDeliveryEvent>,
}

impl ItemDeliveryLedger {
    /// Create an empty item-delivery ledger bound to one immutable release.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryError`] when a public reference, digest, or locale is
    /// malformed.
    pub fn new(
        session_ref: &str,
        instrument_release_ref: &str,
        release_content_digest: &str,
        locale: &str,
    ) -> Result<Self, ItemDeliveryError> {
        let session_ref = required_reference(session_ref)?;
        let instrument_release_ref = required_reference(instrument_release_ref)?;
        if !valid_sha256_digest(release_content_digest) {
            return Err(ItemDeliveryError::InvalidDigest);
        }
        let locale = locale.trim();
        if !valid_locale(locale) {
            return Err(ItemDeliveryError::InvalidLocale);
        }

        Ok(Self {
            session_ref: session_ref.to_owned(),
            instrument_release_ref: instrument_release_ref.to_owned(),
            release_content_digest: release_content_digest.to_owned(),
            locale: locale.to_owned(),
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
    /// immutable event. Reuse of that identity with different evidence fails closed.
    /// A different delivery identity cannot re-administer an item version already
    /// delivered in this session.
    ///
    /// # Errors
    ///
    /// Returns [`ItemDeliveryError::SessionNotActive`] when the session is not active,
    /// [`ItemDeliveryError::InvalidReference`] for malformed evidence references,
    /// [`ItemDeliveryError::IdempotencyConflict`] for conflicting replay, or
    /// [`ItemDeliveryError::DuplicateItemDelivery`] for a second logical delivery of
    /// the same immutable item version.
    pub fn deliver(
        &mut self,
        state: SessionState,
        request: ItemDeliveryRequest<'_>,
    ) -> Result<ItemDeliveryEvent, ItemDeliveryError> {
        if state != SessionState::Active {
            return Err(ItemDeliveryError::SessionNotActive(state));
        }

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
    normalized_reference(reference).ok_or(ItemDeliveryError::InvalidReference)
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
    let Some(primary) = subtags.next() else {
        return false;
    };
    if !(2..=8).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    subtags.all(|subtag| {
        (1..=8).contains(&subtag.len()) && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}
