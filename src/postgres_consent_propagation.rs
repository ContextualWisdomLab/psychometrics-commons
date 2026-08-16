//! Atomic `PostgreSQL` composition for consent evidence and propagation outbox events.
//!
//! Consent remains purpose-specific, append-only product evidence. This module composes the
//! existing consent adapter with the existing transactional outbox and verifies that the emitted
//! event is bound to the authorized tenant, same participant, and latest accepted consent event
//! before any durable write. After the ledger snapshot is persisted, the same transaction locks
//! the participant ledger and requires the durable event tail—ordered by occurrence time, then
//! insertion time, then event identity—to match the envelope, so a grant-only in-memory snapshot
//! cannot pair a later stored revocation with stale grant propagation even when both events share
//! one server timestamp.

use crate::consent::ConsentLedger;
use crate::integration::IntegrationEvent;
use crate::postgres_consent::{
    persist_consent_ledger, ConsentPersistenceDisposition, ConsentPersistenceError,
};
use crate::postgres_integration::{enqueue_outbox_event, PersistenceDisposition, PersistenceError};
use crate::reference::normalized_reference;
use postgres::Transaction;
use std::error::Error;
use std::fmt::{Display, Formatter};

const SOURCE_REF: &str = "psychometrics_commons";

/// Durable dispositions produced by one atomic consent/outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsentOutboxPersistence {
    consent: ConsentPersistenceDisposition,
    outbox: PersistenceDisposition,
}

impl ConsentOutboxPersistence {
    /// Return whether consent evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn consent(self) -> ConsentPersistenceDisposition {
        self.consent
    }

    /// Return whether immutable outbox evidence was newly inserted or exactly replayed.
    #[must_use]
    pub const fn outbox(self) -> PersistenceDisposition {
        self.outbox
    }
}

/// Fail-closed error for atomic consent and outbox persistence.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConsentOutboxPersistenceError {
    /// The propagation envelope does not identify this tenant, participant, and latest ledger event.
    InvalidPropagationEnvelope,
    /// Durable consent evidence failed validation or persistence.
    Consent(ConsentPersistenceError),
    /// Durable integration outbox evidence failed validation or persistence.
    Outbox(PersistenceError),
}

impl Display for ConsentOutboxPersistenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPropagationEnvelope => {
                "consent propagation event must bind the authorized tenant, participant, and latest consent event"
            }
            Self::Consent(_) => "consent propagation consent persistence failed",
            Self::Outbox(_) => "consent propagation outbox persistence failed",
        })
    }
}

impl Error for ConsentOutboxPersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPropagationEnvelope => None,
            Self::Consent(error) => Some(error),
            Self::Outbox(error) => Some(error),
        }
    }
}

/// Persist one consent ledger and its latest causally bound outbox event in the same transaction.
///
/// `authorized_tenant_ref` is the product authorization context resolved by the caller. It must
/// already be in canonical opaque-reference spelling; this boundary never normalizes aliases. The
/// integration event must use that exact tenant, be emitted by `psychometrics_commons`, use the
/// consent ledger's participant as its subject, identify the latest accepted consent event through
/// `causation_ref`, and use that consent event's server-authoritative occurrence time. Binding
/// propagation to tenant, participant, and ledger tail prevents cross-tenant dispatch and prevents
/// a later revocation or grant from being durably paired with stale historical propagation evidence.
/// In-memory envelope checks run before persistence. After the submitted snapshot is written, the
/// same transaction locks `consent_ledger` and requires the durable `consent_event` tail—ordered by
/// occurrence time, then physical insertion time, then event identity—to equal that causation
/// reference and time. Equal server timestamps therefore keep a later-inserted revocation ahead of
/// an earlier grant whose opaque identity sorts later. A caller that omits a later stored
/// revocation or grant therefore fails closed before an outbox row is created.
/// Callers should persist one new consent event per composition so an earlier purpose change is not
/// hidden behind a later event that becomes the only bound outbox. Event type, correlation
/// reference, schema version, and payload digest remain owned by the caller's versioned
/// integration contract.
///
/// The caller owns the `READ COMMITTED` transaction and final commit/rollback decision. If either
/// durable adapter fails, callers must roll the transaction back so newly accepted consent evidence
/// cannot survive without its outbox record. Exact replay remains idempotent at both adapters.
///
/// # Errors
///
/// Returns [`ConsentOutboxPersistenceError::InvalidPropagationEnvelope`] before writes for an
/// invalid, non-canonical, or mismatched authorized tenant, unrelated source or participant, stale
/// or missing causation reference, or timestamp, and after ledger persistence when the durable
/// tail does not match the envelope. Consent and outbox failures are preserved in typed error
/// variants.
pub fn persist_consent_ledger_with_outbox(
    transaction: &mut Transaction<'_>,
    authorized_tenant_ref: &str,
    ledger: &ConsentLedger,
    propagation_event: &IntegrationEvent,
    outbox_max_attempts: usize,
) -> Result<ConsentOutboxPersistence, ConsentOutboxPersistenceError> {
    validate_propagation_envelope(authorized_tenant_ref, ledger, propagation_event)?;
    let consent = persist_consent_ledger(transaction, ledger)
        .map_err(ConsentOutboxPersistenceError::Consent)?;
    require_durable_ledger_tail(transaction, ledger, propagation_event)?;
    let outbox = enqueue_outbox_event(transaction, propagation_event, outbox_max_attempts)
        .map_err(ConsentOutboxPersistenceError::Outbox)?;
    Ok(ConsentOutboxPersistence { consent, outbox })
}

fn validate_propagation_envelope(
    authorized_tenant_ref: &str,
    ledger: &ConsentLedger,
    propagation_event: &IntegrationEvent,
) -> Result<(), ConsentOutboxPersistenceError> {
    let Some(normalized_tenant_ref) = normalized_reference(authorized_tenant_ref) else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    if normalized_tenant_ref != authorized_tenant_ref
        || propagation_event.tenant_ref() != authorized_tenant_ref
        || propagation_event.source() != SOURCE_REF
        || propagation_event.subject_ref() != ledger.participant_ref()
    {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    let Some(causation_ref) = propagation_event.causation_ref() else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    let Some(consent_event) = ledger
        .events()
        .last()
        .filter(|event| event.event_ref() == causation_ref)
    else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    if consent_event.occurred_at_unix_ms() != propagation_event.occurred_at_unix_ms() {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    Ok(())
}

fn require_durable_ledger_tail(
    transaction: &mut Transaction<'_>,
    ledger: &ConsentLedger,
    propagation_event: &IntegrationEvent,
) -> Result<(), ConsentOutboxPersistenceError> {
    let Some(causation_ref) = propagation_event.causation_ref() else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    if transaction
        .query_opt(
            "SELECT participant_ref FROM consent_ledger WHERE participant_ref = $1 FOR UPDATE",
            &[&ledger.participant_ref()],
        )
        .map_err(|error| ConsentOutboxPersistenceError::Consent(error.into()))?
        .is_none()
    {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    let Some(row) = transaction
        .query_opt(
            "SELECT event_ref, occurred_at_unix_ms \
             FROM consent_event \
             WHERE participant_ref = $1 \
             ORDER BY occurred_at_unix_ms DESC, created_at DESC, event_ref DESC \
             LIMIT 1",
            &[&ledger.participant_ref()],
        )
        .map_err(|error| ConsentOutboxPersistenceError::Consent(error.into()))?
    else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    let stored_event_ref: String = row.get(0);
    let stored_occurred: i64 = row.get(1);
    let Ok(expected_occurred) = i64::try_from(propagation_event.occurred_at_unix_ms()) else {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    };
    if stored_event_ref != causation_ref {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    if stored_occurred != expected_occurred {
        return Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope);
    }
    Ok(())
}

#[cfg(test)]
mod envelope_tests {
    use super::{validate_propagation_envelope, ConsentOutboxPersistenceError};
    use crate::consent::{ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose};
    use crate::integration::IntegrationEvent;

    const DIGEST: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const TENANT_REF: &str = "tenant_consent_envelope_unit";

    fn ledger() -> ConsentLedger {
        let mut ledger = ConsentLedger::new("participant_consent_envelope_unit").unwrap();
        ledger
            .record(ConsentEventInput {
                event_ref: "consent_event_envelope_unit",
                purpose: ConsentPurpose::ResearchContribution,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: "consent_form_envelope_unit",
                research_scope_ref: Some("research_scope_envelope_unit"),
                occurred_at_unix_ms: 10_000,
            })
            .unwrap();
        ledger
    }

    fn event(
        source: &str,
        tenant_ref: &str,
        subject: &str,
        causation: Option<&str>,
        occurred_at_unix_ms: u64,
    ) -> IntegrationEvent {
        IntegrationEvent::new(
            "event_consent_envelope_unit",
            "consent.changed",
            "v1",
            source,
            tenant_ref,
            subject,
            occurred_at_unix_ms,
            "correlation_consent_envelope_unit",
            causation,
            DIGEST,
        )
        .unwrap()
    }

    #[test]
    fn every_envelope_binding_boundary_fails_closed() {
        let ledger = ledger();
        let valid = event(
            "psychometrics_commons",
            TENANT_REF,
            ledger.participant_ref(),
            Some("consent_event_envelope_unit"),
            10_000,
        );
        assert!(validate_propagation_envelope(TENANT_REF, &ledger, &valid).is_ok());

        let invalid = [
            (
                "tenant_consent_envelope_other",
                event(
                    "psychometrics_commons",
                    TENANT_REF,
                    ledger.participant_ref(),
                    Some("consent_event_envelope_unit"),
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "psychometrics_commons",
                    "tenant_consent_envelope_other",
                    ledger.participant_ref(),
                    Some("consent_event_envelope_unit"),
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "other_source",
                    TENANT_REF,
                    ledger.participant_ref(),
                    Some("consent_event_envelope_unit"),
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "psychometrics_commons",
                    TENANT_REF,
                    "participant_other",
                    Some("consent_event_envelope_unit"),
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "psychometrics_commons",
                    TENANT_REF,
                    ledger.participant_ref(),
                    None,
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "psychometrics_commons",
                    TENANT_REF,
                    ledger.participant_ref(),
                    Some("consent_event_unknown"),
                    10_000,
                ),
            ),
            (
                TENANT_REF,
                event(
                    "psychometrics_commons",
                    TENANT_REF,
                    ledger.participant_ref(),
                    Some("consent_event_envelope_unit"),
                    10_001,
                ),
            ),
        ];
        for (authorized_tenant_ref, candidate) in invalid {
            assert!(matches!(
                validate_propagation_envelope(authorized_tenant_ref, &ledger, &candidate),
                Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
            ));
        }

        for invalid_tenant_ref in [
            " ",
            "42",
            " tenant_consent_envelope_unit",
            "tenant_consent_envelope_unit ",
        ] {
            assert!(matches!(
                validate_propagation_envelope(invalid_tenant_ref, &ledger, &valid),
                Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
            ));
        }
    }

    #[test]
    fn empty_ledger_cannot_bind_a_propagation_envelope() {
        let empty = ConsentLedger::new("participant_consent_envelope_unit").unwrap();
        let candidate = event(
            "psychometrics_commons",
            TENANT_REF,
            empty.participant_ref(),
            Some("consent_event_envelope_unit"),
            10_000,
        );
        assert!(matches!(
            validate_propagation_envelope(TENANT_REF, &empty, &candidate),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
    }
}

#[cfg(test)]
mod durable_tail_boundary_tests {
    use super::{require_durable_ledger_tail, ConsentOutboxPersistenceError};
    use crate::consent::{ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose};
    use crate::integration::IntegrationEvent;
    use crate::postgres_consent::apply_consent_migration;
    use postgres::{Client, NoTls};

    const DIGEST: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const SCHEMA: &str = "consent_outbox_durable_tail_unit";

    fn ready_client() -> Client {
        let connection = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must identify the isolated CI PostgreSQL database");
        let mut client = Client::connect(&connection, NoTls)
            .expect("isolated CI PostgreSQL database must be reachable");
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;
                 CREATE SCHEMA {SCHEMA};
                 SET search_path TO {SCHEMA};"
            ))
            .unwrap();
        apply_consent_migration(&mut client).unwrap();
        client
    }

    fn grant_ledger() -> ConsentLedger {
        let mut ledger = ConsentLedger::new("participant_consent_durable_tail").unwrap();
        ledger
            .record(ConsentEventInput {
                event_ref: "consent_event_durable_tail_grant",
                purpose: ConsentPurpose::ResearchContribution,
                decision: ConsentDecision::Granted,
                consent_form_version_ref: "consent_form_durable_tail",
                research_scope_ref: Some("research_scope_durable_tail"),
                occurred_at_unix_ms: 40_000,
            })
            .unwrap();
        ledger
    }

    fn event(causation: Option<&str>, occurred_at_unix_ms: u64) -> IntegrationEvent {
        IntegrationEvent::new(
            "event_consent_durable_tail",
            "consent.changed",
            "v1",
            "psychometrics_commons",
            "tenant_consent_durable_tail",
            "participant_consent_durable_tail",
            occurred_at_unix_ms,
            "correlation_consent_durable_tail",
            causation,
            DIGEST,
        )
        .unwrap()
    }

    #[test]
    fn durable_tail_boundaries_fail_closed_without_enqueue() {
        let mut client = ready_client();
        let ledger = grant_ledger();
        let bound = event(Some("consent_event_durable_tail_grant"), 40_000);

        let mut missing_ledger = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(&mut missing_ledger, &ledger, &bound),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        assert!(matches!(
            require_durable_ledger_tail(&mut missing_ledger, &ledger, &event(None, 40_000)),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        missing_ledger.rollback().unwrap();

        client
            .execute(
                "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
                &[&ledger.participant_ref()],
            )
            .unwrap();
        let mut missing_events = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(&mut missing_events, &ledger, &bound),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        missing_events.rollback().unwrap();

        let research_scope_ref = Some("research_scope_durable_tail");
        client
            .execute(
                "INSERT INTO consent_event (\
                     participant_ref, event_ref, consent_purpose, consent_decision, \
                     consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
                 ) VALUES ($1, $2, 'research_contribution', 'granted', $3, $4, 40000)",
                &[
                    &ledger.participant_ref(),
                    &"consent_event_durable_tail_grant",
                    &"consent_form_durable_tail",
                    &research_scope_ref,
                ],
            )
            .unwrap();

        let mut mismatch = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(
                &mut mismatch,
                &ledger,
                &event(Some("consent_event_durable_tail_grant"), 40_001),
            ),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        assert!(matches!(
            require_durable_ledger_tail(
                &mut mismatch,
                &ledger,
                &event(Some("consent_event_durable_tail_other"), 40_000),
            ),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        require_durable_ledger_tail(&mut mismatch, &ledger, &bound)
            .expect("exact durable tail should accept the bound envelope");
        mismatch.rollback().unwrap();

        let mut overflow_transaction = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(
                &mut overflow_transaction,
                &ledger,
                &event(Some("consent_event_durable_tail_grant"), u64::MAX),
            ),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        overflow_transaction.rollback().unwrap();

        client.batch_execute("DROP TABLE consent_event;").unwrap();
        let mut missing_event_relation = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(&mut missing_event_relation, &ledger, &bound),
            Err(ConsentOutboxPersistenceError::Consent(_))
        ));
        missing_event_relation.rollback().unwrap();
        client.batch_execute("DROP TABLE consent_ledger;").unwrap();
        let mut missing_ledger_relation = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(&mut missing_ledger_relation, &ledger, &bound),
            Err(ConsentOutboxPersistenceError::Consent(_))
        ));
        missing_ledger_relation.rollback().unwrap();
        client
            .batch_execute(&format!("DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"))
            .unwrap();
    }

    #[test]
    fn same_millisecond_revoke_is_the_durable_tail() {
        let mut client = ready_client();
        let ledger = grant_ledger();
        let research_scope_ref = Some("research_scope_durable_tail");
        client
            .execute(
                "INSERT INTO consent_ledger (participant_ref) VALUES ($1)",
                &[&ledger.participant_ref()],
            )
            .unwrap();
        client
            .execute(
                "INSERT INTO consent_event (\
                     participant_ref, event_ref, consent_purpose, consent_decision, \
                     consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
                 ) VALUES ($1, $2, 'research_contribution', 'granted', $3, $4, 40000)",
                &[
                    &ledger.participant_ref(),
                    &"consent_event_durable_tail_grant",
                    &"consent_form_durable_tail",
                    &research_scope_ref,
                ],
            )
            .unwrap();
        client
            .execute(
                "INSERT INTO consent_event (\
                     participant_ref, event_ref, consent_purpose, consent_decision, \
                     consent_form_version_ref, research_scope_ref, occurred_at_unix_ms\
                 ) VALUES ($1, $2, 'research_contribution', 'revoked', $3, $4, 40000)",
                &[
                    &ledger.participant_ref(),
                    &"consent_event_aaa_durable_tail_revoke",
                    &"consent_form_durable_tail",
                    &research_scope_ref,
                ],
            )
            .unwrap();

        let mut transaction = client.transaction().unwrap();
        assert!(matches!(
            require_durable_ledger_tail(
                &mut transaction,
                &ledger,
                &event(Some("consent_event_durable_tail_grant"), 40_000),
            ),
            Err(ConsentOutboxPersistenceError::InvalidPropagationEnvelope)
        ));
        require_durable_ledger_tail(
            &mut transaction,
            &ledger,
            &event(Some("consent_event_aaa_durable_tail_revoke"), 40_000),
        )
        .expect("later-inserted same-millisecond revoke should be the durable tail");
        transaction.rollback().unwrap();
        client
            .batch_execute(&format!("DROP SCHEMA IF EXISTS {SCHEMA} CASCADE;"))
            .unwrap();
    }
}
