//! Identity-boundary regressions for consent and research contribution.

use psychometrics_commons_runtime::consent::{
    ConsentDecision, ConsentEventInput, ConsentLedger, ConsentPurpose, ConsentWriteError,
    ResearchContribution,
};

#[test]
fn opaque_product_references_reject_numeric_only_identifiers() {
    assert_eq!(
        ConsentLedger::new("12345"),
        Err(ConsentWriteError::EmptyReference)
    );
    assert_eq!(
        ConsentLedger::new(" participant_ref "),
        Err(ConsentWriteError::EmptyReference)
    );
    assert!(ConsentLedger::new("participant_12345").is_ok());
}

#[test]
fn research_identity_cannot_reuse_operational_participant_identity() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_ref").unwrap();

    let error = ResearchContribution::from_snapshot(
        "contribution_ref",
        "participant_ref",
        &snapshot,
        11_000,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "research participant reference must differ from the operational participant"
    );
}

#[test]
fn research_identity_rejects_whitespace_padded_aliases() {
    let mut ledger = ConsentLedger::new("participant_ref").unwrap();
    ledger
        .record(ConsentEventInput {
            event_ref: "research_grant",
            purpose: ConsentPurpose::ResearchContribution,
            decision: ConsentDecision::Granted,
            consent_form_version_ref: "research_form_v1",
            research_scope_ref: Some("research_scope_v1"),
            occurred_at_unix_ms: 11_000,
        })
        .unwrap();
    let snapshot = ledger.snapshot_as("consent_snapshot_ref").unwrap();

    let error = ResearchContribution::from_snapshot(
        "contribution_ref",
        " research_participant_ref ",
        &snapshot,
        11_000,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "research contribution references must not be empty"
    );
}
