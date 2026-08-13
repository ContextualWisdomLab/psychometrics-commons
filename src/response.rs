//! Response-event ledger and immutable response-snapshot semantics.
//!
//! The hosted runtime accepts new logical response events only while a session is
//! active. Exact client-event replays remain idempotent after the collection
//! lifecycle advances, while client event references provide replay detection and
//! server event references plus monotonic sequences preserve a stable audit order.
//! Completing a session freezes the accepted response prefix into an immutable
//! snapshot value.

use crate::reference::normalized_reference;
use crate::session::SessionState;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Borrowed input used to record one response event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponseWrite<'a> {
    /// Server-generated opaque event reference reserved for a new event.
    pub server_event_ref: &'a str,
    /// Client-generated idempotency reference for replay detection.
    pub client_event_ref: &'a str,
    /// Exact immutable item-version reference answered by the participant.
    pub item_version_ref: &'a str,
    /// Digest identifying the canonical response payload without logging it.
    pub payload_digest: &'a str,
}

/// One accepted response event in server-authoritative sequence order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseEvent {
    server_event_ref: String,
    client_event_ref: String,
    item_version_ref: String,
    payload_digest: String,
    sequence: usize,
}

impl ResponseEvent {
    /// Return the server-generated opaque event reference.
    #[must_use]
    pub fn server_event_ref(&self) -> &str {
        &self.server_event_ref
    }

    /// Return the client idempotency reference.
    #[must_use]
    pub fn client_event_ref(&self) -> &str {
        &self.client_event_ref
    }

    /// Return the immutable item-version reference.
    #[must_use]
    pub fn item_version_ref(&self) -> &str {
        &self.item_version_ref
    }

    /// Return the canonical response-payload digest.
    #[must_use]
    pub fn payload_digest(&self) -> &str {
        &self.payload_digest
    }

    /// Return the server-assigned monotonic sequence number.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }
}

/// Immutable response snapshot frozen when collection completes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseSnapshot {
    snapshot_ref: Option<String>,
    session_ref: String,
    event_refs: Vec<String>,
    item_version_refs: Vec<String>,
    payload_digests: Vec<String>,
    last_sequence: Option<usize>,
}

impl ResponseSnapshot {
    /// Return the durable snapshot reference when one was assigned at freeze time.
    #[must_use]
    pub fn snapshot_ref(&self) -> Option<&str> {
        self.snapshot_ref.as_deref()
    }

    /// Return the opaque assessment-session reference.
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// Return the number of response events frozen into this snapshot.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.event_refs.len()
    }

    /// Return the last accepted server sequence, if the snapshot is non-empty.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<usize> {
        self.last_sequence
    }

    /// Return response-event references in server-authoritative order.
    #[must_use]
    pub fn event_refs(&self) -> &[String] {
        &self.event_refs
    }

    /// Return item-version references aligned with [`Self::event_refs`].
    #[must_use]
    pub fn item_version_refs(&self) -> &[String] {
        &self.item_version_refs
    }

    /// Return response-payload digests aligned with [`Self::event_refs`].
    #[must_use]
    pub fn payload_digests(&self) -> &[String] {
        &self.payload_digests
    }
}

/// Fail-closed error returned by response recording or snapshot freezing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WriteError {
    /// A new logical response was offered while the session was not active.
    SessionNotActive(SessionState),
    /// An identity-bearing response or snapshot reference was blank or numeric-like.
    InvalidReference,
    /// A required response-payload digest was blank.
    EmptyReference,
    /// A nonblank response-payload digest was not canonical lowercase SHA-256 evidence.
    InvalidPayloadDigest,
    /// A client idempotency reference was reused for different response content.
    IdempotencyConflict,
    /// A server event reference was reused for a distinct response event.
    ServerReferenceConflict,
    /// A response snapshot was requested before the session reached completion.
    SnapshotRequiresCompleted(SessionState),
}

impl Display for WriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotActive(state) => {
                write!(formatter, "session {state:?} cannot accept response events")
            }
            Self::InvalidReference => formatter
                .write_str("response identity references must be opaque non-numeric values"),
            Self::EmptyReference => {
                formatter.write_str("response payload digest must not be empty")
            }
            Self::InvalidPayloadDigest => formatter.write_str(
                "response payload digest must be canonical lowercase sha256 evidence",
            ),
            Self::IdempotencyConflict => formatter.write_str(
                "client event reference was already used for different response content",
            ),
            Self::ServerReferenceConflict => formatter
                .write_str("server event reference was already used by another response event"),
            Self::SnapshotRequiresCompleted(state) => write!(
                formatter,
                "response snapshot requires Completed session state, found {state:?}"
            ),
        }
    }
}

impl Error for WriteError {}

/// In-memory domain ledger defining response idempotency and snapshot behavior.
///
/// Persistence adapters may store events differently, but they must preserve
/// the semantics expressed by this type: server-monotonic sequence order,
/// client-reference idempotency, and immutable completed-session snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseLedger {
    session_ref: String,
    events: Vec<ResponseEvent>,
}

impl ResponseLedger {
    /// Create an empty response ledger for one assessment session.
    ///
    /// Leading and trailing whitespace is removed before the session reference
    /// becomes identity-bearing state.
    ///
    /// # Errors
    ///
    /// Returns [`WriteError::InvalidReference`] when the session reference is blank
    /// or numeric-like instead of an opaque product identifier.
    pub fn new(session_ref: impl AsRef<str>) -> Result<Self, WriteError> {
        let session_ref =
            normalized_reference(session_ref.as_ref()).ok_or(WriteError::InvalidReference)?;
        Ok(Self {
            session_ref: session_ref.to_owned(),
            events: Vec::new(),
        })
    }

    /// Return the number of accepted response events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Return whether no response event has been accepted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Record one response event or replay an identical prior event.
    ///
    /// Exact replay of an already accepted `client_event_ref` remains idempotent
    /// even after the session leaves [`SessionState::Active`]. The supplied
    /// server event reference is ignored for that replay because the original
    /// immutable event identity is returned. Every genuinely new logical response
    /// still requires an active session. Identity-bearing references are normalized
    /// before replay/conflict checks so surrounding whitespace cannot create aliases.
    /// Response-payload identity must use exact `sha256:` plus 64 lowercase hexadecimal
    /// characters, matching the durable PostgreSQL digest constraint.
    ///
    /// # Errors
    ///
    /// Returns [`WriteError::InvalidReference`] for blank or numeric-like identity
    /// references, [`WriteError::EmptyReference`] for a blank payload digest,
    /// [`WriteError::InvalidPayloadDigest`] for a noncanonical digest,
    /// [`WriteError::IdempotencyConflict`] when a client event reference is reused
    /// with different item or payload content, [`WriteError::SessionNotActive`]
    /// when a new logical response is offered outside active collection, or
    /// [`WriteError::ServerReferenceConflict`] when a server event reference
    /// already identifies another response event.
    pub fn record(
        &mut self,
        state: SessionState,
        request: ResponseWrite<'_>,
    ) -> Result<ResponseEvent, WriteError> {
        let server_event_ref =
            normalized_reference(request.server_event_ref).ok_or(WriteError::InvalidReference)?;
        let client_event_ref =
            normalized_reference(request.client_event_ref).ok_or(WriteError::InvalidReference)?;
        let item_version_ref =
            normalized_reference(request.item_version_ref).ok_or(WriteError::InvalidReference)?;
        let payload_digest = request.payload_digest;
        if payload_digest.trim().is_empty() {
            return Err(WriteError::EmptyReference);
        }
        if !is_canonical_sha256(payload_digest) {
            return Err(WriteError::InvalidPayloadDigest);
        }

        if let Some(existing) = self
            .events
            .iter()
            .find(|event| event.client_event_ref == client_event_ref)
        {
            if existing.item_version_ref == item_version_ref
                && existing.payload_digest == payload_digest
            {
                return Ok(existing.clone());
            }
            return Err(WriteError::IdempotencyConflict);
        }

        if !state.accepts_responses() {
            return Err(WriteError::SessionNotActive(state));
        }

        if self
            .events
            .iter()
            .any(|event| event.server_event_ref == server_event_ref)
        {
            return Err(WriteError::ServerReferenceConflict);
        }

        let event = ResponseEvent {
            server_event_ref: server_event_ref.to_owned(),
            client_event_ref: client_event_ref.to_owned(),
            item_version_ref: item_version_ref.to_owned(),
            payload_digest: payload_digest.to_owned(),
            sequence: self.events.len() + 1,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Freeze the accepted event prefix without assigning durable persistence identity.
    ///
    /// This method is useful for purely in-memory inspection. Scoring dispatch
    /// deliberately rejects such unbound snapshots; persistence adapters must use
    /// [`Self::freeze_as`] when the snapshot becomes durable.
    ///
    /// # Errors
    ///
    /// Returns [`WriteError::SnapshotRequiresCompleted`] unless `state` is
    /// exactly [`SessionState::Completed`].
    pub fn freeze(&self, state: SessionState) -> Result<ResponseSnapshot, WriteError> {
        self.freeze_internal(state, None)
    }

    /// Freeze the accepted event prefix with its durable opaque snapshot identity.
    ///
    /// Leading and trailing whitespace is removed before the reference becomes
    /// identity-bearing state.
    ///
    /// # Errors
    ///
    /// Returns [`WriteError::InvalidReference`] for a blank or numeric-like snapshot
    /// reference or [`WriteError::SnapshotRequiresCompleted`] unless `state` is
    /// exactly [`SessionState::Completed`].
    pub fn freeze_as(
        &self,
        state: SessionState,
        snapshot_ref: &str,
    ) -> Result<ResponseSnapshot, WriteError> {
        let snapshot_ref =
            normalized_reference(snapshot_ref).ok_or(WriteError::InvalidReference)?;
        self.freeze_internal(state, Some(snapshot_ref))
    }

    fn freeze_internal(
        &self,
        state: SessionState,
        snapshot_ref: Option<&str>,
    ) -> Result<ResponseSnapshot, WriteError> {
        if state != SessionState::Completed {
            return Err(WriteError::SnapshotRequiresCompleted(state));
        }

        Ok(ResponseSnapshot {
            snapshot_ref: snapshot_ref.map(str::to_owned),
            session_ref: self.session_ref.clone(),
            event_refs: self
                .events
                .iter()
                .map(|event| event.server_event_ref.clone())
                .collect(),
            item_version_refs: self
                .events
                .iter()
                .map(|event| event.item_version_ref.clone())
                .collect(),
            payload_digests: self
                .events
                .iter()
                .map(|event| event.payload_digest.clone())
                .collect(),
            last_sequence: self.events.last().map(ResponseEvent::sequence),
        })
    }
}

fn is_canonical_sha256(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
