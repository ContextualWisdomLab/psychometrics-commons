//! Anonymous assessment sessions must authorize their own consent before any durable write.

use psychometrics_commons_runtime::anonymous_session::AnonymousSessionContext;
use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose,
};
use psychometrics_commons_runtime::postgres_consent_authorization::{
    authorize_anonymous_consent_propagation, AuthorizedConsentPersistenceError,
};
use std::error::Error;

const TENANT_REF: &str = "tenant_consent_anonymous_alpha";
const PARTICIPANT_REF: &str = "participant_consent_anonymous_alpha";
const SESSION_REF: &str = "session_consent_anonymous_alpha";
const EVIDENCE_REF: &str = "evidence_consent_anonymous_alpha";
const VALID_UNTIL_UNIX_MS: u64 = 80_000;

fn owner_anonymous_session() -> AnonymousSessionContext {
    AnonymousSessionContext::new(
        TENANT_REF,
        PARTICIPANT_REF,
        SESSION_REF,
        EVIDENCE_REF,
        VALID_UNTIL_UNIX_MS,
    )
    .unwrap()
}

fn research_grant_ledger() -> ConsentLedger {
    let mut ledger = ConsentLedger::new(PARTICIPANT_REF).unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_anonymous_write_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_anonymous_write_v1",
            research_scope_ref: Some("research_scope_anonymous_alpha"),
            occurred_at_unix_ms: 40_000,
        })
        .unwrap();
    ledger
}

#[test]
fn current_anonymous_session_may_authorize_own_research_grant() {
    let ledger = research_grant_ledger();
    authorize_anonymous_consent_propagation(&owner_anonymous_session(), &ledger, 79_999).unwrap();
    let snapshot = ledger
        .snapshot_as("consent_snapshot_anonymous_write_grant")
        .unwrap();
    assert!(snapshot.is_granted(ConsentPurpose::ResearchContribution));
    assert!(!snapshot.is_granted(ConsentPurpose::ServiceOperation));
}

#[test]
fn expired_anonymous_session_cannot_authorize_consent_propagation() {
    let error = authorize_anonymous_consent_propagation(
        &owner_anonymous_session(),
        &research_grant_ledger(),
        VALID_UNTIL_UNIX_MS,
    )
    .expect_err("expired anonymous session must fail closed");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::AnonymousSessionExpired
    ));
    assert!(error
        .to_string()
        .contains("start or resume the assessment, then record consent again"));
    assert!(error.source().is_none());
}

#[test]
fn unknown_anonymous_session_time_cannot_authorize_consent_propagation() {
    let error = authorize_anonymous_consent_propagation(
        &owner_anonymous_session(),
        &research_grant_ledger(),
        0,
    )
    .expect_err("unknown time must fail closed");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::AnonymousSessionExpired
    ));
}

#[test]
fn anonymous_session_cannot_authorize_a_foreign_consent_ledger() {
    let mut foreign_ledger = ConsentLedger::new("participant_consent_anonymous_other").unwrap();
    foreign_ledger
        .record(ConsentEventInput {
            event_ref: "consent_event_anonymous_write_foreign",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "consent_form_anonymous_write_v1",
            research_scope_ref: Some("research_scope_anonymous_alpha"),
            occurred_at_unix_ms: 40_000,
        })
        .unwrap();
    let error = authorize_anonymous_consent_propagation(
        &owner_anonymous_session(),
        &foreign_ledger,
        40_000,
    )
    .expect_err("foreign ledger must fail closed");
    assert!(matches!(
        error,
        AuthorizedConsentPersistenceError::AnonymousBindingMismatch
    ));
    assert!(error
        .to_string()
        .contains("open the matching assessment, then record consent there"));
    assert!(error.source().is_none());
}
