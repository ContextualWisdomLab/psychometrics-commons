//! Product consent writes must authorize the owning participant before any durable work.

use psychometrics_commons_runtime::authorization::{
    AuthorizationContext, AuthorizationError, ProductRole,
};
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent_authorization::{
    authorize_consent_propagation, AuthorizedConsentPersistenceError,
};
use std::error::Error;

const TENANT_REF: &str = "tenant_consent_write_alpha";
const PARTICIPANT_REF: &str = "participant_consent_write_alpha";

fn owner_context() -> AuthorizationContext {
    AuthorizationContext::new(
        TENANT_REF,
        "subject_consent_write_alpha",
        Some(PARTICIPANT_REF),
        &[ProductRole::Participant],
    )
    .unwrap()
}

fn research_grant_ledger() -> ConsentLedger {
    let mut ledger = ConsentLedger::new(PARTICIPANT_REF).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_write_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_write_v1",
            research_scope_ref: Some("research_scope_write_alpha"),
            occurred_at_unix_ms: 40_000,
        })
        .unwrap();
    ledger
}

fn service_grant_ledger() -> ConsentLedger {
    let mut ledger = ConsentLedger::new(PARTICIPANT_REF).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_write_service",
            purpose: ConsentPurpose::ServiceOperation,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_write_v1",
            research_scope_ref: None,
            occurred_at_unix_ms: 40_000,
        })
        .unwrap();
    ledger
}

#[test]
fn owner_may_authorize_own_research_grant_without_implying_other_purposes() {
    let ledger = research_grant_ledger();
    authorize_consent_propagation(&owner_context(), &ledger, TENANT_REF).unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_write_grant").unwrap();
    assert!(snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert!(!snapshot.is_granted(ConsentPurpose::ServiceOperation));
}

#[test]
fn service_grant_does_not_create_research_contribution() {
    let ledger = service_grant_ledger();
    authorize_consent_propagation(&owner_context(), &ledger, TENANT_REF).unwrap();
    let snapshot = ledger
        .snapshot_as("consent_snapshot_write_service")
        .unwrap();
    assert!(snapshot.is_granted(ConsentPurpose::ServiceOperation));
    assert!(!snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert_eq!(snapshot.active_research_scope(), None);
}

#[test]
fn other_participant_cannot_authorize_a_foreign_consent_ledger() {
    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_consent_write_intruder",
        Some("participant_consent_write_other"),
        &[ProductRole::Participant],
    )
    .unwrap();
    let error = authorize_consent_propagation(&actor, &research_grant_ledger(), TENANT_REF)
        .expect_err("foreign participant must not authorize another ledger");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::Authorization(AuthorizationError::OwnerMismatch)
    ));
}

#[test]
fn cross_tenant_actor_cannot_authorize_consent_propagation() {
    let actor = AuthorizationContext::new(
        "tenant_consent_write_beta",
        "subject_consent_write_alpha",
        Some(PARTICIPANT_REF),
        &[ProductRole::Participant],
    )
    .unwrap();
    let error = authorize_consent_propagation(&actor, &research_grant_ledger(), TENANT_REF)
        .expect_err("foreign tenant must not authorize this ledger");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::Authorization(AuthorizationError::CrossTenantDenied)
    ));
}

#[test]
fn actor_without_participant_identity_cannot_authorize_consent_propagation() {
    let actor = AuthorizationContext::new(
        TENANT_REF,
        "subject_consent_write_alpha",
        None,
        &[ProductRole::Participant],
    )
    .unwrap();
    let error = authorize_consent_propagation(&actor, &research_grant_ledger(), TENANT_REF)
        .expect_err("missing participant identity must fail closed");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::Authorization(
            AuthorizationError::ParticipantIdentityRequired
        )
    ));
}

#[test]
fn numeric_tenant_is_rejected_before_any_consent_write() {
    let error = authorize_consent_propagation(&owner_context(), &research_grant_ledger(), "123")
        .expect_err("numeric tenant must fail closed");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::Authorization(AuthorizationError::InvalidReference)
    ));
    assert!(error
        .to_string()
        .contains("authenticated participant to manage their own ledger"));
    assert!(error.source().is_some());
}
